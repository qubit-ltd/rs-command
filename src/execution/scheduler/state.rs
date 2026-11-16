use super::{
    reservation::Kind,
    waiter::Waiters,
    worker::{Queued, WorkerQueue},
};
use crate::execution::{
    CommandExecutionConfig,
    contracts::{CapacityFailure, EnqueueFailure, Reservation, ScheduledJob, Scheduler},
};
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex},
    task::Waker,
    time::Instant,
};

#[derive(Clone, Copy)]
struct Record {
    kind: Kind,
    bytes: usize,
}
struct State {
    closed: bool,
    next: u64,
    active: usize,
    queued: usize,
    input_bytes: usize,
    residual_helpers: usize,
    records: HashMap<u64, Record>,
    waiters: Waiters,
    workers: WorkerQueue,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    config: CommandExecutionConfig,
}

/// Thread-safe scheduler with independently bounded active, queued, waiter and byte budgets.
#[derive(Clone)]
pub struct BoundedScheduler(Arc<Shared>);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerSnapshot {
    pub active: usize,
    pub queued: usize,
    pub waiters: usize,
    pub input_bytes: usize,
    pub residual_helpers: usize,
}
impl BoundedScheduler {
    pub fn new(
        config: CommandExecutionConfig,
    ) -> Result<Self, crate::execution::CommandExecutorCreateError> {
        config.validate()?;
        Ok(Self(Arc::new(Shared {
            state: Mutex::new(State {
                closed: false,
                next: 0,
                active: 0,
                queued: 0,
                input_bytes: 0,
                residual_helpers: 0,
                records: HashMap::new(),
                waiters: Waiters::default(),
                workers: WorkerQueue::default(),
            }),
            changed: Condvar::new(),
            config,
        })))
    }
    pub fn snapshot(&self) -> SchedulerSnapshot {
        let s = self.0.state.lock().unwrap();
        SchedulerSnapshot {
            active: s.active,
            queued: s.queued,
            waiters: s.waiters.len(),
            input_bytes: s.input_bytes,
            residual_helpers: s.residual_helpers,
        }
    }
    pub fn register_waiter(&self, waker: Option<&Waker>) -> Result<u64, CapacityFailure> {
        let mut s = self.0.state.lock().unwrap();
        if s.closed {
            return Err(CapacityFailure::Closed);
        }
        s.waiters
            .insert(waker, self.0.config.max_waiters)
            .ok_or(CapacityFailure::QueueFull)
    }
    pub fn update_waiter(&self, id: u64, w: &Waker) {
        let mut s = self.0.state.lock().unwrap();
        s.waiters.update(id, w);
    }
    pub fn cancel_waiter(&self, id: u64) {
        let mut s = self.0.state.lock().unwrap();
        s.waiters.remove(id);
        drop(s);
        self.0.changed.notify_all();
    }
    pub fn wait_for_capacity(&self, deadline: Option<Instant>) -> bool {
        let mut s = self.0.state.lock().unwrap();
        loop {
            if s.closed {
                return false;
            }
            if s.active < self.0.config.max_active || s.queued < self.0.config.max_queued {
                return true;
            }
            match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return false;
                    }
                    let (next, t) = self.0.changed.wait_timeout(s, d - now).unwrap();
                    s = next;
                    if t.timed_out() {
                        return false;
                    }
                }
                None => s = self.0.changed.wait(s).unwrap(),
            }
        }
    }
    pub fn take_next(&self) -> Option<ScheduledJob> {
        let mut s = self.0.state.lock().unwrap();
        let active_pos = s.workers.jobs.iter().position(|q| {
            s.records
                .get(&q.permit.token)
                .is_some_and(|r| r.kind == Kind::Active)
        });
        let position = active_pos.or_else(|| {
            (s.active < self.0.config.max_active && !s.workers.jobs.is_empty()).then_some(0)
        })?;
        let mut q = s.workers.jobs.remove(position)?;
        let token = q.permit.token;
        if s.records.get(&token)?.kind == Kind::Queued {
            if s.active >= self.0.config.max_active {
                s.workers.jobs.push_front(q);
                return None;
            }
            s.queued -= 1;
            s.active += 1;
            if let Some(r) = s.records.get_mut(&token) {
                r.kind = Kind::Active
            }
        }
        let run = q.job.run;
        let permit = q.permit;
        q.job.run = Box::new(move || {
            run();
            drop(permit)
        });
        Some(q.job)
    }
    fn release(shared: &Arc<Shared>, token: u64) {
        let mut s = shared.state.lock().unwrap();
        if let Some(r) = s.records.remove(&token) {
            match r.kind {
                Kind::Active => s.active = s.active.saturating_sub(1),
                Kind::Queued => s.queued = s.queued.saturating_sub(1),
            }
            s.input_bytes = s.input_bytes.saturating_sub(r.bytes);
        }
        let wakes = s.waiters.wake_all();
        drop(s);
        for w in wakes {
            w.wake()
        }
        shared.changed.notify_all();
    }
}
impl Scheduler for BoundedScheduler {
    fn try_reserve(&self, input_bytes: usize) -> Result<Reservation, CapacityFailure> {
        let mut s = self.0.state.lock().unwrap();
        if s.closed {
            return Err(CapacityFailure::Closed);
        }
        let bytes = s
            .input_bytes
            .checked_add(input_bytes)
            .filter(|b| *b <= self.0.config.max_queued_input_bytes)
            .ok_or(CapacityFailure::InputBytesExceeded)?;
        let kind = if s.active < self.0.config.max_active {
            s.active += 1;
            Kind::Active
        } else if s.queued < self.0.config.max_queued {
            s.queued += 1;
            Kind::Queued
        } else {
            return Err(if self.0.config.max_queued == 0 {
                CapacityFailure::ActiveFull
            } else {
                CapacityFailure::QueueFull
            });
        };
        s.input_bytes = bytes;
        s.next = s.next.wrapping_add(1).max(1);
        let token = s.next;
        s.records.insert(
            token,
            Record {
                kind,
                bytes: input_bytes,
            },
        );
        let shared = Arc::clone(&self.0);
        Ok(Reservation::new(token, move |id| {
            Self::release(&shared, id)
        }))
    }
    fn enqueue(&self, job: ScheduledJob, permit: Reservation) -> Result<(), EnqueueFailure> {
        let mut s = self.0.state.lock().unwrap();
        let token = permit.token;
        if s.closed {
            return Err(EnqueueFailure {
                reason: CapacityFailure::Closed,
                job,
                permit,
            });
        }
        if !s.records.contains_key(&token) {
            return Err(EnqueueFailure {
                reason: CapacityFailure::Closed,
                job,
                permit,
            });
        }
        s.workers.jobs.push_back(Queued { job, permit });
        Ok(())
    }
    fn close_admission(&self) {
        let mut s = self.0.state.lock().unwrap();
        s.closed = true;
        let wakes = s.waiters.wake_all();
        drop(s);
        for w in wakes {
            w.wake()
        }
        self.0.changed.notify_all();
    }
}
