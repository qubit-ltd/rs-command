use super::{
    reservation::Kind,
    waiter::Waiters,
    worker::{Queued, WorkerQueue},
};
use crate::execution::{
    CommandExecutionConfig,
    contracts::{
        CapacityFailure, EnqueueFailure, Reservation, ReservationLease, ScheduledJob, Scheduler,
    },
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::Waker,
    time::Instant,
};

static NEXT_SCHEDULER_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy)]
struct Record {
    kind: Kind,
    bytes: usize,
    deadline: Option<Instant>,
    owner_live: bool,
    leases: usize,
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
    id: u64,
    state: Mutex<State>,
    changed: Condvar,
    config: CommandExecutionConfig,
}

/// Thread-safe bounded scheduler. Its internal queue and dispatch threads are bounded by the
/// configured active and queued permits; worker threads are owned and started by the executor.
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
        let id = NEXT_SCHEDULER_ID.fetch_add(1, Ordering::Relaxed);
        Ok(Self(Arc::new(Shared {
            id,
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
    pub fn register_waiter_if_full(
        &self,
        input_bytes: usize,
        waker: &Waker,
    ) -> Result<Option<u64>, CapacityFailure> {
        let mut s = self.0.state.lock().unwrap();
        if s.closed {
            return Err(CapacityFailure::Closed);
        }
        if available(&s, &self.0.config, input_bytes) {
            return Ok(None);
        }
        Ok(Some(
            s.waiters
                .insert(Some(waker), self.0.config.max_waiters)
                .ok_or(CapacityFailure::QueueFull)?,
        ))
    }
    pub fn update_waiter(&self, id: u64, w: &Waker) {
        let mut s = self.0.state.lock().unwrap();
        s.waiters.update(id, w)
    }
    pub fn cancel_waiter(&self, id: u64) {
        let mut s = self.0.state.lock().unwrap();
        s.waiters.remove(id);
        drop(s);
        self.0.changed.notify_all()
    }
    pub fn wait_for_capacity(&self, input_bytes: usize, deadline: Option<Instant>) -> bool {
        let mut s = self.0.state.lock().unwrap();
        let waiter_id = if available(&s, &self.0.config, input_bytes) {
            None
        } else {
            match s.waiters.insert(None, self.0.config.max_waiters) {
                Some(id) => Some(id),
                None => return false,
            }
        };
        let result = loop {
            if s.closed {
                break false;
            }
            if available(&s, &self.0.config, input_bytes) {
                break true;
            }
            match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        break false;
                    }
                    let (next, t) = self.0.changed.wait_timeout(s, d - now).unwrap();
                    s = next;
                    if t.timed_out() && !available(&s, &self.0.config, input_bytes) {
                        break false;
                    }
                }
                None => s = self.0.changed.wait(s).unwrap(),
            }
        };
        if let Some(id) = waiter_id {
            s.waiters.remove(id);
        }
        result
    }
    /// Concrete entry point for submissions carrying their original, inherited deadline.
    pub fn try_reserve_until(
        &self,
        input_bytes: usize,
        deadline: Option<Instant>,
    ) -> Result<Reservation, CapacityFailure> {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(CapacityFailure::DeadlineExpired);
        }
        self.reserve(input_bytes, deadline)
    }
    fn reserve(
        &self,
        input_bytes: usize,
        deadline: Option<Instant>,
    ) -> Result<Reservation, CapacityFailure> {
        let mut s = self.0.state.lock().unwrap();
        if s.closed {
            return Err(CapacityFailure::Closed);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(CapacityFailure::DeadlineExpired);
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
                deadline,
                owner_live: true,
                leases: 0,
            },
        );
        let shared = Arc::clone(&self.0);
        let release_shared = shared.clone();
        Ok(Reservation::new(
            shared.id,
            token,
            move |id| BoundedScheduler::release_owner(&release_shared, id),
            move |id| retain_lease(&shared, id),
        ))
    }
    /// Retrieves immediately when possible. `wait_next` is preferred by worker threads.
    pub fn try_next(&self) -> Option<(ScheduledJob, Reservation)> {
        loop {
            let mut s = self.0.state.lock().unwrap();
            let expired = s.workers.jobs.iter().position(|q| {
                s.records
                    .get(&q.permit.token)
                    .and_then(|r| r.deadline)
                    .is_some_and(|d| Instant::now() >= d)
            });
            if let Some(pos) = expired {
                let q = s.workers.jobs.remove(pos);
                drop(s);
                drop(q);
                continue;
            }
            let pos = next_job_position(&s, &self.0.config);
            let mut q = s.workers.jobs.remove(pos?)?;
            promote(&mut s, &mut q);
            let wakes = s.waiters.wake_all();
            drop(s);
            for w in wakes {
                w.wake()
            }
            self.0.changed.notify_all();
            return Some((q.job, q.permit));
        }
    }
    fn release_owner(shared: &Arc<Shared>, token: u64) {
        let mut s = shared.state.lock().unwrap();
        if let Some(r) = s.records.get_mut(&token) {
            r.owner_live = false;
            if r.leases == 0 {
                remove_record(&mut s, token)
            }
        }
        notify_capacity(shared, s)
    }
    fn release_lease(shared: &Arc<Shared>, token: u64) {
        let mut s = shared.state.lock().unwrap();
        let remove = if let Some(r) = s.records.get_mut(&token) {
            r.leases = r.leases.saturating_sub(1);
            !r.owner_live && r.leases == 0
        } else {
            false
        };
        s.residual_helpers = s.residual_helpers.saturating_sub(1);
        if remove {
            remove_record(&mut s, token)
        }
        notify_capacity(shared, s)
    }
}
fn available(s: &State, c: &CommandExecutionConfig, bytes: usize) -> bool {
    s.input_bytes
        .checked_add(bytes)
        .is_some_and(|n| n <= c.max_queued_input_bytes)
        && (s.active < c.max_active || s.queued < c.max_queued)
}
fn remove_record(s: &mut State, token: u64) {
    if let Some(r) = s.records.remove(&token) {
        match r.kind {
            Kind::Active => s.active = s.active.saturating_sub(1),
            Kind::Queued => s.queued = s.queued.saturating_sub(1),
        }
        s.input_bytes = s.input_bytes.saturating_sub(r.bytes)
    }
}
fn notify_capacity(shared: &Arc<Shared>, mut s: std::sync::MutexGuard<'_, State>) {
    let wakes = s.waiters.wake_all();
    drop(s);
    for w in wakes {
        w.wake()
    }
    shared.changed.notify_all()
}
fn retain_lease(shared: &Arc<Shared>, token: u64) -> ReservationLease {
    let mut s = shared.state.lock().unwrap();
    if let Some(r) = s.records.get_mut(&token) {
        r.leases += 1;
        s.residual_helpers += 1
    }
    let shared = Arc::clone(shared);
    ReservationLease::new(move || BoundedScheduler::release_lease(&shared, token))
}
fn next_job_position(s: &State, c: &CommandExecutionConfig) -> Option<usize> {
    s.workers.jobs.iter().position(|q| {
        s.records.get(&q.permit.token).is_some_and(|r| {
            !r.deadline.is_some_and(|d| Instant::now() >= d)
                && (r.kind == Kind::Active || s.active < c.max_active)
        })
    })
}
fn promote(s: &mut State, q: &mut Queued) {
    let token = q.permit.token;
    if let Some(r) = s.records.get_mut(&token) {
        if r.kind == Kind::Queued {
            s.queued = s.queued.saturating_sub(1);
            s.active += 1;
            r.kind = Kind::Active
        }
    }
}
impl Scheduler for BoundedScheduler {
    fn try_reserve(&self, input_bytes: usize) -> Result<Reservation, CapacityFailure> {
        self.reserve(input_bytes, None)
    }
    fn enqueue(&self, job: ScheduledJob, permit: Reservation) -> Result<(), EnqueueFailure> {
        let mut s = self.0.state.lock().unwrap();
        if s.closed {
            return Err(EnqueueFailure {
                reason: CapacityFailure::Closed,
                job,
                permit,
            });
        }
        if !permit.belongs_to(self.0.id) {
            return Err(EnqueueFailure {
                reason: CapacityFailure::ForeignReservation,
                job,
                permit,
            });
        }
        if !s.records.contains_key(&permit.token) {
            return Err(EnqueueFailure {
                reason: CapacityFailure::Closed,
                job,
                permit,
            });
        }
        if s.records[&permit.token]
            .deadline
            .is_some_and(|d| Instant::now() >= d)
        {
            return Err(EnqueueFailure {
                reason: CapacityFailure::DeadlineExpired,
                job,
                permit,
            });
        }
        s.workers.jobs.push_back(Queued { job, permit });
        drop(s);
        self.0.changed.notify_all();
        Ok(())
    }
    fn wait_next(&self) -> Option<(ScheduledJob, Reservation)> {
        let mut s = self.0.state.lock().unwrap();
        loop {
            let expired_pos = s.workers.jobs.iter().position(|q| {
                s.records
                    .get(&q.permit.token)
                    .and_then(|r| r.deadline)
                    .is_some_and(|d| Instant::now() >= d)
            });
            if let Some(pos) = expired_pos {
                let q = s.workers.jobs.remove(pos);
                drop(s);
                drop(q);
                s = self.0.state.lock().unwrap();
                continue;
            }
            if let Some(pos) = next_job_position(&s, &self.0.config) {
                let mut q = s.workers.jobs.remove(pos)?;
                promote(&mut s, &mut q);
                let wakes = s.waiters.wake_all();
                drop(s);
                for w in wakes {
                    w.wake()
                }
                self.0.changed.notify_all();
                return Some((q.job, q.permit));
            }
            if s.closed && s.records.is_empty() {
                return None;
            }
            let next_deadline = s
                .workers
                .jobs
                .iter()
                .filter_map(|q| s.records.get(&q.permit.token).and_then(|r| r.deadline))
                .min();
            if let Some(d) = next_deadline {
                let now = Instant::now();
                let (ns, _) = self.0.changed.wait_timeout(s, d - now).unwrap();
                s = ns
            } else {
                s = self.0.changed.wait(s).unwrap()
            }
        }
    }
    fn close_admission(&self) {
        let mut s = self.0.state.lock().unwrap();
        s.closed = true;
        let wakes = s.waiters.wake_all();
        drop(s);
        for w in wakes {
            w.wake()
        }
        self.0.changed.notify_all()
    }
}
