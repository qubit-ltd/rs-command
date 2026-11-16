// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::task::Waker;

use qubit_operation::OperationContext;
use qubit_operation::StopReason;

use super::internal::Record;
use super::internal::Shared;
use super::kind::Kind;
use super::waiters::Waiters;
use super::worker::Queued;
use super::worker::WorkerQueue;
use crate::execution::CommandExecutionConfig;
use crate::execution::contracts::CapacityFailure;
use crate::execution::contracts::EnqueueFailure;
use crate::execution::contracts::Reservation;
use crate::execution::contracts::ReservationLease;
use crate::execution::contracts::ScheduledJob;
use crate::execution::contracts::Scheduler;
static NEXT_SCHEDULER_ID: AtomicU64 = AtomicU64::new(1);
const CLOCK_RECHECK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

use super::SchedulerSnapshot;
use super::internal::State;

/// Thread-safe bounded scheduler. Its internal queue and dispatch threads are
/// bounded by the configured active and queued permits; worker threads are
/// owned and started by the executor.
/// Represents the bounded scheduler state or contract used by managed command
/// execution.
#[derive(Clone)]
pub struct BoundedScheduler(#[doc = "Shared queue and permit state owned by this scheduler."] Arc<Shared>);
impl BoundedScheduler {
    /// Performs the new step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `config`: Supplies the config value used by this operation.
    pub fn new(config: CommandExecutionConfig) -> Result<Self, crate::execution::CommandExecutorCreateError> {
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
    /// Performs the snapshot step in the managed command execution lifecycle.
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
    /// Stores the register waiter value used by this execution component.
    ///
    /// # Parameters
    /// * `waker`: Supplies the waker value used by this operation.
    pub fn register_waiter(&self, waker: Option<&Waker>) -> Result<u64, CapacityFailure> {
        let mut s = self.0.state.lock().unwrap();
        if s.closed {
            return Err(CapacityFailure::Closed);
        }
        s.waiters
            .insert(waker, self.0.config.max_waiters)
            .ok_or(CapacityFailure::QueueFull)
    }
    /// Performs the register waiter if full step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    /// * `waker`: Supplies the waker value used by this operation.
    pub fn register_waiter_if_full(&self, input_bytes: usize, waker: &Waker) -> Result<Option<u64>, CapacityFailure> {
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
    /// Stores the update waiter value used by this execution component.
    ///
    /// # Parameters
    /// * `id`: Supplies the id value used by this operation.
    /// * `w`: Supplies the w value used by this operation.
    pub fn update_waiter(&self, id: u64, w: &Waker) {
        let mut s = self.0.state.lock().unwrap();
        s.waiters.update(id, w)
    }
    /// Stores the cancel waiter value used by this execution component.
    ///
    /// # Parameters
    /// * `id`: Supplies the id value used by this operation.
    pub fn cancel_waiter(&self, id: u64) {
        let mut s = self.0.state.lock().unwrap();
        s.waiters.remove(id);
        drop(s);
        self.0.changed.notify_all()
    }
    /// Performs the wait for capacity step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    pub fn wait_for_capacity(&self, input_bytes: usize, deadline: Option<OperationContext>) -> bool {
        let mut s = self.0.state.lock().unwrap();
        if deadline.as_ref().is_some_and(OperationContext::is_expired) {
            return false;
        }
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
            if deadline.as_ref().is_some_and(OperationContext::is_expired) {
                break false;
            }
            if available(&s, &self.0.config, input_bytes) {
                break true;
            }
            if deadline.as_ref().is_some_and(|context| context.deadline().is_some()) {
                let (next, _) = self.0.changed.wait_timeout(s, CLOCK_RECHECK_INTERVAL).unwrap();
                s = next;
            } else {
                s = self.0.changed.wait(s).unwrap();
            }
        };
        if let Some(id) = waiter_id {
            s.waiters.remove(id);
        }
        result
    }
    /// Concrete entry point for submissions carrying their original, inherited
    /// deadline.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    pub fn try_reserve_until(
        &self,
        input_bytes: usize,
        deadline: Option<OperationContext>,
    ) -> Result<Reservation, CapacityFailure> {
        if deadline.as_ref().is_some_and(OperationContext::is_expired) {
            return Err(CapacityFailure::DeadlineExpired);
        }
        self.reserve(input_bytes, deadline)
    }
    /// Performs the reserve step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    fn reserve(&self, input_bytes: usize, deadline: Option<OperationContext>) -> Result<Reservation, CapacityFailure> {
        let mut s = self.0.state.lock().unwrap();
        if s.closed {
            return Err(CapacityFailure::Closed);
        }
        if deadline.as_ref().is_some_and(OperationContext::is_expired) {
            return Err(CapacityFailure::DeadlineExpired);
        }
        promote_queued(&mut s, &self.0.config);
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
    /// Retrieves immediately when possible. `wait_next` is preferred by worker
    /// threads.
    pub fn try_next(&self) -> Option<(ScheduledJob, Reservation)> {
        loop {
            let mut s = self.0.state.lock().unwrap();
            if let Some((token, stop)) = request_expired_stop(&mut s) {
                drop(s);
                stop.request_stop(StopReason::Timeout);
                timeout_stop_delivered(&self.0, token);
                continue;
            }
            let pos = next_job_position(&s, &self.0.config);
            let mut q = s.workers.jobs.remove(pos?)?;
            if is_expired(&s, &q) && !q.timeout_requested {
                q.timeout_requested = true;
                q.timeout_pending = true;
                let token = q.permit.token;
                let stop = Arc::clone(&q.job.stop);
                s.workers.jobs.insert(pos?, q);
                drop(s);
                stop.request_stop(StopReason::Timeout);
                timeout_stop_delivered(&self.0, token);
                continue;
            }
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
    /// Stores the release owner value used by this execution component.
    ///
    /// # Parameters
    /// * `shared`: Supplies the shared value used by this operation.
    /// * `token`: Supplies the token value used by this operation.
    fn release_owner(shared: &Arc<Shared>, token: u64) {
        let mut s = shared.state.lock().unwrap();
        let mut freed_active = false;
        if let Some(r) = s.records.get_mut(&token) {
            r.owner_live = false;
            if r.leases == 0 {
                freed_active = remove_record(&mut s, token)
            }
        }
        if freed_active {
            promote_queued(&mut s, &shared.config)
        }
        notify_capacity(shared, s)
    }
    /// Stores the release lease value used by this execution component.
    ///
    /// # Parameters
    /// * `shared`: Supplies the shared value used by this operation.
    /// * `token`: Supplies the token value used by this operation.
    fn release_lease(shared: &Arc<Shared>, token: u64) {
        let mut s = shared.state.lock().unwrap();
        let remove = if let Some(r) = s.records.get_mut(&token) {
            r.leases = r.leases.saturating_sub(1);
            !r.owner_live && r.leases == 0
        } else {
            false
        };
        s.residual_helpers = s.residual_helpers.saturating_sub(1);
        let freed_active = if remove { remove_record(&mut s, token) } else { false };
        if freed_active {
            promote_queued(&mut s, &shared.config)
        }
        notify_capacity(shared, s)
    }
}
/// Stores the available value used by this execution component.
///
/// # Parameters
/// * `s`: Supplies the s value used by this operation.
/// * `c`: Supplies the c value used by this operation.
/// * `bytes`: Supplies the bytes value used by this operation.
fn available(s: &State, c: &CommandExecutionConfig, bytes: usize) -> bool {
    s.input_bytes
        .checked_add(bytes)
        .is_some_and(|n| n <= c.max_queued_input_bytes)
        && (s.active < c.max_active || s.queued < c.max_queued)
}
/// Stores the remove record value used by this execution component.
///
/// # Parameters
/// * `s`: Supplies the s value used by this operation.
/// * `token`: Supplies the token value used by this operation.
fn remove_record(s: &mut State, token: u64) -> bool {
    if let Some(r) = s.records.remove(&token) {
        let was_active = r.kind == Kind::Active;
        match r.kind {
            Kind::Active => s.active = s.active.saturating_sub(1),
            Kind::Queued => s.queued = s.queued.saturating_sub(1),
        }
        s.input_bytes = s.input_bytes.saturating_sub(r.bytes);
        was_active
    } else {
        false
    }
}
/// Stores the promote queued value used by this execution component.
///
/// # Parameters
/// * `s`: Supplies the s value used by this operation.
/// * `config`: Supplies the config value used by this operation.
fn promote_queued(s: &mut State, config: &CommandExecutionConfig) {
    while s.active < config.max_active {
        let next = s
            .records
            .iter()
            .filter(|(_, r)| r.kind == Kind::Queued)
            .map(|(token, _)| *token)
            .min();
        let Some(token) = next else { break };
        if let Some(r) = s.records.get_mut(&token) {
            r.kind = Kind::Active;
            s.queued = s.queued.saturating_sub(1);
            s.active += 1;
        }
    }
}
/// Stores the is expired value used by this execution component.
///
/// # Parameters
/// * `s`: Supplies the s value used by this operation.
/// * `q`: Supplies the q value used by this operation.
fn is_expired(s: &State, q: &Queued) -> bool {
    s.records
        .get(&q.permit.token)
        .and_then(|r| r.deadline.as_ref())
        .is_some_and(OperationContext::is_expired)
}
/// Stores the request expired stop value used by this execution component.
///
/// # Parameters
/// * `s`: Supplies the s value used by this operation.
fn request_expired_stop(s: &mut State) -> Option<(u64, Arc<dyn crate::execution::StopPort>)> {
    let pos = s
        .workers
        .jobs
        .iter()
        .position(|q| !q.timeout_requested && is_expired(s, q))?;
    let q = s.workers.jobs.get_mut(pos)?;
    q.timeout_requested = true;
    q.timeout_pending = true;
    Some((q.permit.token, Arc::clone(&q.job.stop)))
}
/// Stores the timeout stop delivered value used by this execution component.
///
/// # Parameters
/// * `shared`: Supplies the shared value used by this operation.
/// * `token`: Supplies the token value used by this operation.
fn timeout_stop_delivered(shared: &Arc<Shared>, token: u64) {
    let mut s = shared.state.lock().unwrap();
    if let Some(q) = s.workers.jobs.iter_mut().find(|q| q.permit.token == token) {
        q.timeout_pending = false;
    }
    drop(s);
    shared.changed.notify_all();
}
/// Stores the notify capacity value used by this execution component.
///
/// # Parameters
/// * `shared`: Supplies the shared value used by this operation.
/// * `s`: Supplies the s value used by this operation.
fn notify_capacity(shared: &Arc<Shared>, mut s: std::sync::MutexGuard<'_, State>) {
    let wakes = s.waiters.wake_all();
    drop(s);
    for w in wakes {
        w.wake()
    }
    shared.changed.notify_all()
}
/// Stores the retain lease value used by this execution component.
///
/// # Parameters
/// * `shared`: Supplies the shared value used by this operation.
/// * `token`: Supplies the token value used by this operation.
fn retain_lease(shared: &Arc<Shared>, token: u64) -> ReservationLease {
    let mut s = shared.state.lock().unwrap();
    if let Some(r) = s.records.get_mut(&token) {
        r.leases += 1;
        s.residual_helpers += 1
    }
    let shared = Arc::clone(shared);
    ReservationLease::new(move || BoundedScheduler::release_lease(&shared, token))
}
/// Stores the next job position value used by this execution component.
///
/// # Parameters
/// * `s`: Supplies the s value used by this operation.
/// * `c`: Supplies the c value used by this operation.
fn next_job_position(s: &State, c: &CommandExecutionConfig) -> Option<usize> {
    let oldest_active = s
        .workers
        .jobs
        .iter()
        .enumerate()
        .filter_map(|(position, q)| {
            s.records
                .get(&q.permit.token)
                .filter(|r| r.kind == Kind::Active)
                .map(|_| (position, q.permit.token, q.timeout_pending))
        })
        .min_by_key(|(_, token, _)| *token);
    let (position, _, timeout_pending) = if let Some(active) = oldest_active {
        active
    } else if s.active < c.max_active {
        s.workers
            .jobs
            .iter()
            .enumerate()
            .filter_map(|(position, q)| {
                s.records
                    .get(&q.permit.token)
                    .filter(|r| r.kind == Kind::Queued)
                    .map(|_| (position, q.permit.token, q.timeout_pending))
            })
            .min_by_key(|(_, token, _)| *token)?
    } else {
        return None;
    };
    // A job awaiting its Timeout callback retains its FIFO position; later accepted
    // jobs cannot overtake it while the callback is running outside the state lock.
    if timeout_pending {
        return None;
    }
    Some(position)
}
/// Stores the promote value used by this execution component.
///
/// # Parameters
/// * `s`: Supplies the s value used by this operation.
/// * `q`: Supplies the q value used by this operation.
fn promote(s: &mut State, q: &mut Queued) {
    let token = q.permit.token;
    if let Some(r) = s.records.get_mut(&token)
        && r.kind == Kind::Queued
    {
        s.queued = s.queued.saturating_sub(1);
        s.active += 1;
        r.kind = Kind::Active
    }
}
impl Scheduler for BoundedScheduler {
    /// Stores the try reserve value used by this execution component.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    fn try_reserve(&self, input_bytes: usize) -> Result<Reservation, CapacityFailure> {
        self.reserve(input_bytes, None)
    }
    /// Performs the try reserve until step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    fn try_reserve_until(
        &self,
        input_bytes: usize,
        deadline: Option<OperationContext>,
    ) -> Result<Reservation, CapacityFailure> {
        BoundedScheduler::try_reserve_until(self, input_bytes, deadline)
    }
    /// Stores the enqueue value used by this execution component.
    ///
    /// # Parameters
    /// * `job`: Supplies the job value used by this operation.
    /// * `permit`: Supplies the permit value used by this operation.
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
            .as_ref()
            .is_some_and(OperationContext::is_expired)
        {
            return Err(EnqueueFailure {
                reason: CapacityFailure::DeadlineExpired,
                job,
                permit,
            });
        }
        s.workers.jobs.push_back(Queued {
            job,
            permit,
            timeout_requested: false,
            timeout_pending: false,
        });
        drop(s);
        self.0.changed.notify_all();
        Ok(())
    }
    /// Performs the wait next step in the managed command execution lifecycle.
    fn wait_next(&self) -> Option<(ScheduledJob, Reservation)> {
        let mut s = self.0.state.lock().unwrap();
        loop {
            if let Some((token, stop)) = request_expired_stop(&mut s) {
                drop(s);
                stop.request_stop(StopReason::Timeout);
                timeout_stop_delivered(&self.0, token);
                s = self.0.state.lock().unwrap();
                continue;
            }
            if let Some(pos) = next_job_position(&s, &self.0.config) {
                let mut q = s.workers.jobs.remove(pos)?;
                if is_expired(&s, &q) && !q.timeout_requested {
                    q.timeout_requested = true;
                    q.timeout_pending = true;
                    let token = q.permit.token;
                    let stop = Arc::clone(&q.job.stop);
                    s.workers.jobs.insert(pos, q);
                    drop(s);
                    stop.request_stop(StopReason::Timeout);
                    timeout_stop_delivered(&self.0, token);
                    s = self.0.state.lock().unwrap();
                    continue;
                }
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
            let has_deadline = s.workers.jobs.iter().any(|q| {
                s.records
                    .get(&q.permit.token)
                    .is_some_and(|r| r.deadline.as_ref().is_some_and(|context| context.deadline().is_some()))
            });
            if has_deadline {
                let (ns, _) = self.0.changed.wait_timeout(s, CLOCK_RECHECK_INTERVAL).unwrap();
                s = ns
            } else {
                s = self.0.changed.wait(s).unwrap()
            }
        }
    }
    /// Performs the close admission step in the managed command execution
    /// lifecycle.
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
