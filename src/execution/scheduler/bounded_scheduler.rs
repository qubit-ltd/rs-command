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

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::Duration;
    use std::time::Instant;

    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::TimerRegistration;

    use super::BoundedScheduler;
    use crate::execution::CommandExecutionConfig;
    use crate::execution::contracts::CapacityFailure;
    use crate::execution::contracts::ScheduledJob;
    use crate::execution::contracts::Scheduler;

    struct CountWake(AtomicUsize);

    struct ManualDeadlineClock {
        domain: ClockDomainId,
        ticks: AtomicU64,
    }

    impl ClockSource for ManualDeadlineClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.domain, self.ticks.load(Ordering::SeqCst))
        }

        fn register_deadline(&self, _deadline: MonotonicTime, _waker: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    struct NoopRegistration;

    impl TimerRegistration for NoopRegistration {
        fn cancel(self: Box<Self>) {}
    }

    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn config(max_queued: usize, max_waiters: usize) -> CommandExecutionConfig {
        CommandExecutionConfig {
            max_active: 1,
            max_queued,
            max_waiters,
            max_queued_input_bytes: 8,
            max_diagnostics: 1,
        }
    }

    #[test]
    fn test_replaced_capacity_waiter_wakes_when_active_reservation_releases() {
        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let permit = scheduler.try_reserve(3).unwrap();
        let old = Arc::new(CountWake(AtomicUsize::new(0)));
        let replacement = Arc::new(CountWake(AtomicUsize::new(0)));
        let old_waker = Waker::from(old.clone());
        let replacement_waker = Waker::from(replacement.clone());
        let id = scheduler.register_waiter(Some(&old_waker)).unwrap();

        scheduler.update_waiter(id, &replacement_waker);
        scheduler.update_waiter(id + 1, &old_waker);
        drop(permit);

        assert_eq!(old.0.load(Ordering::SeqCst), 0);
        assert_eq!(replacement.0.load(Ordering::SeqCst), 1);
        scheduler.cancel_waiter(id);
        assert_eq!(scheduler.snapshot().waiters, 0);
        assert_eq!(scheduler.snapshot().active, 0);
        assert_eq!(scheduler.snapshot().input_bytes, 0);
    }

    #[test]
    fn test_waiter_cancellation_and_closed_admission_reject_registrations() {
        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());

        assert_eq!(scheduler.register_waiter_if_full(0, &waker), Ok(None));
        let permit = scheduler.try_reserve(0).unwrap();
        let id = scheduler.register_waiter_if_full(0, &waker).unwrap().unwrap();
        assert_eq!(scheduler.snapshot().waiters, 1);
        scheduler.cancel_waiter(id);
        drop(permit);
        assert_eq!(counter.0.load(Ordering::SeqCst), 0);
        assert_eq!(scheduler.snapshot().waiters, 0);

        scheduler.close_admission();
        assert_eq!(scheduler.register_waiter(None), Err(CapacityFailure::Closed));
        assert_eq!(
            scheduler.register_waiter_if_full(0, &waker),
            Err(CapacityFailure::Closed)
        );
        assert!(!scheduler.wait_for_capacity(0, None));
    }

    #[test]
    fn test_blocking_capacity_waiter_is_removed_when_admission_closes() {
        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let permit = scheduler.try_reserve(0).unwrap();
        let waiting = scheduler.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            done_tx.send(waiting.wait_for_capacity(0, None)).unwrap();
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while scheduler.snapshot().waiters == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.snapshot().waiters, 1);
        scheduler.close_admission();

        assert!(!done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        thread.join().unwrap();
        assert_eq!(scheduler.snapshot().waiters, 0);
        drop(permit);
    }

    #[test]
    fn test_closing_admission_rejects_and_unregisters_all_blocked_waiters() {
        let waiter_count = 3;
        let scheduler = BoundedScheduler::new(config(0, waiter_count)).unwrap();
        let permit = scheduler.try_reserve(0).unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let mut threads = Vec::new();

        for _ in 0..waiter_count {
            let waiting = scheduler.clone();
            let done = done_tx.clone();
            threads.push(std::thread::spawn(move || {
                done.send(waiting.wait_for_capacity(0, None)).unwrap();
            }));
        }
        drop(done_tx);

        let registration_deadline = Instant::now() + Duration::from_secs(2);
        while scheduler.snapshot().waiters != waiter_count && Instant::now() < registration_deadline {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.snapshot().waiters, waiter_count);

        scheduler.close_admission();
        for _ in 0..waiter_count {
            assert!(!done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("closing admission should wake every capacity waiter"));
        }
        for thread in threads {
            thread.join().expect("capacity waiter should exit after close");
        }
        assert_eq!(scheduler.snapshot().waiters, 0);
        assert_eq!(scheduler.snapshot().active, 1);
        drop(permit);
        assert_eq!(scheduler.snapshot().active, 0);
    }

    #[test]
    fn test_queued_reservation_promotes_then_releases_capacity_and_bytes() {
        let scheduler = BoundedScheduler::new(config(1, 1)).unwrap();
        let active = scheduler.try_reserve(5).unwrap();
        let queued = scheduler.try_reserve(2).unwrap();
        assert_eq!(
            (
                scheduler.snapshot().active,
                scheduler.snapshot().queued,
                scheduler.snapshot().input_bytes,
            ),
            (1, 1, 7)
        );

        drop(active);
        assert_eq!(
            (
                scheduler.snapshot().active,
                scheduler.snapshot().queued,
                scheduler.snapshot().input_bytes,
            ),
            (1, 0, 2)
        );
        drop(queued);
        assert_eq!(
            (
                scheduler.snapshot().active,
                scheduler.snapshot().queued,
                scheduler.snapshot().input_bytes,
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn test_reservation_without_inherited_deadline_releases_on_owner_drop() {
        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let reservation = scheduler.try_reserve_until(6, None).unwrap();
        assert_eq!((scheduler.snapshot().active, scheduler.snapshot().input_bytes,), (1, 6));

        drop(reservation);
        assert_eq!((scheduler.snapshot().active, scheduler.snapshot().input_bytes,), (0, 0));
    }

    #[test]
    fn test_reservation_bytes_remain_until_last_lease_is_dropped() {
        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let reservation = scheduler.try_reserve(6).unwrap();
        let lease = reservation.retain();

        drop(reservation);
        assert_eq!(scheduler.snapshot().input_bytes, 6);

        drop(lease);
        assert_eq!(scheduler.snapshot().input_bytes, 0);
    }

    #[test]
    fn test_reservation_rejections_report_byte_and_queue_limits() {
        let scheduler = BoundedScheduler::new(config(1, 1)).unwrap();
        let active = scheduler.try_reserve(8).unwrap();
        assert_eq!(
            scheduler.try_reserve(1).unwrap_err(),
            CapacityFailure::InputBytesExceeded
        );
        drop(active);

        let active = scheduler.try_reserve(0).unwrap();
        let queued = scheduler.try_reserve(0).unwrap();
        assert_eq!(scheduler.try_reserve(0).unwrap_err(), CapacityFailure::QueueFull);

        scheduler.close_admission();
        assert_eq!(scheduler.try_reserve(0).unwrap_err(), CapacityFailure::Closed);
        drop((active, queued));
    }

    #[test]
    fn test_enqueue_deadline_rejection_returns_job_and_reservation() {
        use std::sync::atomic::AtomicBool;

        let scheduler = BoundedScheduler::new(config(1, 1)).unwrap();
        let clock = Arc::new(ManualDeadlineClock {
            domain: ClockDomainId::new_unique(),
            ticks: AtomicU64::new(0),
        });
        let context = OperationContext::new(ContextOptions {
            clock: Some(Arc::clone(&clock) as Arc<dyn ClockSource>),
            deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 5))),
            budget: None,
        })
        .unwrap();
        let mut deadline_signal = Box::pin(context.deadline_signal().expect("context should have a deadline"));
        let waker = Waker::noop().clone();
        let mut task_context = Context::from_waker(&waker);
        assert!(deadline_signal.as_mut().poll(&mut task_context).is_pending());
        let permit = scheduler
            .try_reserve_until(3, Some(context))
            .expect("reservation should succeed before its deadline");
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = Arc::clone(&invoked);
        let job = ScheduledJob {
            run: Box::new(move |_| {
                callback_invoked.store(true, Ordering::SeqCst);
            }),
            stop: Arc::new(NoopStop),
        };

        clock.ticks.store(5, Ordering::SeqCst);
        assert!(matches!(
            deadline_signal.as_mut().poll(&mut task_context),
            Poll::Ready(())
        ));
        let failure = scheduler
            .enqueue(job, permit)
            .expect_err("expired reservation should be rejected during enqueue");

        assert_eq!(failure.reason, CapacityFailure::DeadlineExpired);
        assert!(!invoked.load(Ordering::SeqCst));
        assert_eq!(
            (
                scheduler.snapshot().active,
                scheduler.snapshot().queued,
                scheduler.snapshot().input_bytes
            ),
            (1, 0, 3),
            "enqueue rejection must return the reservation with its accounted input bytes",
        );
        (failure.job.run)(failure.permit);
        assert!(
            invoked.load(Ordering::SeqCst),
            "rejected enqueue must return the original job"
        );
        assert_eq!((scheduler.snapshot().active, scheduler.snapshot().input_bytes), (0, 0));
    }

    #[test]
    fn test_waiter_limit_and_release_notification_preserve_capacity() {
        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let permit = scheduler.try_reserve(0).unwrap();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());

        let waiter_id = scheduler.register_waiter(Some(&waker)).unwrap();
        assert_eq!(scheduler.register_waiter(None), Err(CapacityFailure::QueueFull));
        drop(permit);
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.snapshot().waiters, 1);
        scheduler.cancel_waiter(waiter_id);
        assert_eq!(scheduler.snapshot().waiters, 0);
        assert_eq!(scheduler.register_waiter_if_full(0, &waker), Ok(None));
    }

    #[test]
    fn test_blocking_capacity_wait_succeeds_after_release_notification() {
        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let permit = scheduler.try_reserve(0).unwrap();
        let waiting = scheduler.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            done_tx.send(waiting.wait_for_capacity(0, None)).unwrap();
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while scheduler.snapshot().waiters == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.snapshot().waiters, 1);
        drop(permit);

        assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        thread.join().unwrap();
        assert_eq!(scheduler.snapshot().waiters, 0);
    }

    struct NoopStop;

    impl crate::execution::contracts::StopPort for NoopStop {
        fn request_stop(&self, _reason: qubit_operation::StopReason) {}
    }

    #[test]
    fn test_try_next_dispatches_enqueued_job_and_releases_its_permit() {
        use std::sync::atomic::AtomicBool;

        use crate::execution::contracts::ScheduledJob;
        use crate::execution::contracts::Scheduler;

        let scheduler = BoundedScheduler::new(config(0, 1)).unwrap();
        let permit = scheduler.try_reserve(3).unwrap();
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = Arc::clone(&invoked);
        scheduler
            .enqueue(
                ScheduledJob {
                    run: Box::new(move |_permit| {
                        callback_invoked.store(true, Ordering::SeqCst);
                    }),
                    stop: Arc::new(NoopStop),
                },
                permit,
            )
            .unwrap();

        let (job, permit) = scheduler.try_next().expect("enqueued job should be available");
        (job.run)(permit);

        assert!(invoked.load(Ordering::SeqCst));
        assert_eq!(scheduler.snapshot().active, 0);
        assert_eq!(scheduler.snapshot().input_bytes, 0);
        assert!(scheduler.try_next().is_none());
    }

    #[test]
    fn test_scheduler_trait_dispatches_queued_work_and_drains_after_close() {
        let scheduler = BoundedScheduler::new(config(1, 0)).unwrap();
        let executed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&executed);
        let job = ScheduledJob {
            run: Box::new(move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
            }),
            stop: Arc::new(NoopStop),
        };
        drop(Scheduler::try_reserve_until(&scheduler, 0, None).unwrap());
        let permit = Scheduler::try_reserve(&scheduler, 0).unwrap();

        Scheduler::enqueue(&scheduler, job, permit).unwrap();
        let (job, permit) = Scheduler::wait_next(&scheduler).expect("queued work should be dispatched");
        (job.run)(permit);
        assert_eq!(executed.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.snapshot().active, 0);

        Scheduler::close_admission(&scheduler);
        assert!(Scheduler::wait_next(&scheduler).is_none());
    }
}
