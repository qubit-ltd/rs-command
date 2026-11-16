// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::task::Wake;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;

use qubit_command::execution::CapacityFailure;
use qubit_command::execution::CommandExecutionConfig;
use qubit_command::execution::ScheduledJob;
use qubit_command::execution::Scheduler;
use qubit_command::execution::StopPort;
use qubit_command::execution::scheduler::BoundedScheduler;
use qubit_operation::ClockDomainId;
use qubit_operation::ClockSource;
use qubit_operation::ContextOptions;
use qubit_operation::Deadline;
use qubit_operation::MonotonicTime;
use qubit_operation::OperationContext;
use qubit_operation::StopReason;
use qubit_operation::TimerRegistration;
struct ManualClock {
    domain: ClockDomainId,
    ticks: AtomicU64,
}
impl ManualClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            domain: ClockDomainId::new_unique(),
            ticks: AtomicU64::new(0),
        })
    }
    fn advance_to(&self, ticks: u64) {
        self.ticks.store(ticks, Ordering::SeqCst);
    }
    fn context(self: &Arc<Self>, deadline: u64) -> OperationContext {
        let source: Arc<dyn ClockSource> = self.clone();
        OperationContext::new(ContextOptions {
            clock: Some(source),
            deadline: Some(Deadline::at(MonotonicTime::new(self.domain, deadline))),
            budget: None,
        })
        .unwrap()
    }
}
struct NoopRegistration;
impl TimerRegistration for NoopRegistration {
    fn cancel(self: Box<Self>) {}
}
impl ClockSource for ManualClock {
    fn now(&self) -> MonotonicTime {
        MonotonicTime::new(self.domain, self.ticks.load(Ordering::SeqCst))
    }
    fn register_deadline(&self, _: MonotonicTime, _: Waker) -> Box<dyn TimerRegistration> {
        Box::new(NoopRegistration)
    }
}
struct Stop;
impl StopPort for Stop {
    fn request_stop(&self, _: StopReason) {}
}
struct RecordingStop(Arc<std::sync::Mutex<Vec<StopReason>>>);
impl StopPort for RecordingStop {
    fn request_stop(&self, reason: StopReason) {
        self.0.lock().unwrap().push(reason);
    }
}
struct ChannelStop(mpsc::Sender<StopReason>);
impl StopPort for ChannelStop {
    fn request_stop(&self, reason: StopReason) {
        let _ = self.0.send(reason);
    }
}
struct GatedStop {
    requested: mpsc::Sender<StopReason>,
    gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}
impl StopPort for GatedStop {
    fn request_stop(&self, reason: StopReason) {
        let _ = self.requested.send(reason);
        let (released, changed) = &*self.gate;
        let mut released = released.lock().unwrap();
        while !*released {
            released = changed.wait(released).unwrap();
        }
    }
}
struct Counter(AtomicUsize);
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn config(active: usize, queued: usize, waiters: usize, bytes: usize) -> CommandExecutionConfig {
    CommandExecutionConfig {
        max_active: active,
        max_queued: queued,
        max_waiters: waiters,
        max_queued_input_bytes: bytes,
        max_diagnostics: 4,
    }
}
fn job(f: impl FnOnce(qubit_command::execution::Reservation) + Send + 'static) -> ScheduledJob {
    ScheduledJob {
        run: Box::new(f),
        stop: Arc::new(Stop),
    }
}

#[test]
fn test_capacity_bounds_and_raii_release() {
    let s = BoundedScheduler::new(config(4, 64, 64, 1024)).unwrap();
    let mut permits = Vec::new();
    for _ in 0..68 {
        permits.push(s.try_reserve(1).unwrap())
    }
    let x = s.snapshot();
    assert_eq!((x.active, x.queued, x.input_bytes), (4, 64, 68));
    assert_eq!(s.try_reserve(1).unwrap_err(), CapacityFailure::QueueFull);
    drop(permits.remove(0));
    let x = s.snapshot();
    assert_eq!((x.active, x.queued, x.input_bytes), (4, 63, 67));
}
#[test]
fn test_zero_queue_only_admits_active_slots() {
    let s = BoundedScheduler::new(config(1, 0, 0, 9)).unwrap();
    let p = s.try_reserve(9).unwrap();
    assert_eq!(s.try_reserve(0).unwrap_err(), CapacityFailure::ActiveFull);
    drop(p);
    assert!(s.try_reserve(0).is_ok());
}
#[test]
fn test_input_bytes_precede_slots_and_are_in_capacity_wait_predicate() {
    let s = BoundedScheduler::new(config(1, 2, 2, 2)).unwrap();
    let p = s.try_reserve(2).unwrap();
    assert_eq!(s.try_reserve(1).unwrap_err(), CapacityFailure::InputBytesExceeded);
    let waiting = s.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || tx.send(waiting.wait_for_capacity(1, None)).unwrap());
    drop(p);
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
}
#[test]
fn test_waiter_registration_cancel_and_release_are_race_safe() {
    let s = BoundedScheduler::new(config(1, 0, 2, 8)).unwrap();
    let p = s.try_reserve(0).unwrap();
    let counter = Arc::new(Counter(AtomicUsize::new(0)));
    let w = Waker::from(counter.clone());
    let id = s.register_waiter_if_full(0, &w).unwrap().unwrap();
    drop(p);
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    s.cancel_waiter(id);
    assert_eq!(s.snapshot().waiters, 0);
    assert!(s.register_waiter_if_full(0, &w).unwrap().is_none());
}
#[test]
fn test_foreign_permit_with_colliding_token_is_rejected_and_returned() {
    let a = BoundedScheduler::new(config(1, 1, 1, 9)).unwrap();
    let b = BoundedScheduler::new(config(1, 1, 1, 9)).unwrap();
    let own_active = a.try_reserve(1).unwrap();
    let own_queued = a.try_reserve(2).unwrap();
    let origin_active = b.try_reserve(3).unwrap();
    let foreign = b.try_reserve(4).unwrap();
    let failure = a.enqueue(job(drop), foreign).unwrap_err();
    assert_eq!(failure.reason, CapacityFailure::ForeignReservation);
    let a_before = a.snapshot();
    let b_before = b.snapshot();
    assert_eq!((a_before.active, a_before.queued, a_before.input_bytes), (1, 1, 3));
    assert_eq!((b_before.active, b_before.queued, b_before.input_bytes), (1, 1, 7));
    drop(failure);
    let a_after = a.snapshot();
    let b_after = b.snapshot();
    assert_eq!((a_after.active, a_after.queued, a_after.input_bytes), (1, 1, 3));
    assert_eq!((b_after.active, b_after.queued, b_after.input_bytes), (1, 0, 3));
    drop(own_queued);
    drop(own_active);
    drop(origin_active);
}
#[test]
fn test_multi_job_queue_promotes_only_after_active_release() {
    let s = BoundedScheduler::new(config(1, 2, 1, 8)).unwrap();
    let first = s.try_reserve(0).unwrap();
    let second = s.try_reserve(0).unwrap();
    let third = s.try_reserve(0).unwrap();
    let (tx, rx) = mpsc::channel();
    s.enqueue(job(move |_| tx.send(1).unwrap()), first).unwrap();
    let (tx, rx2) = mpsc::channel();
    s.enqueue(job(move |_| tx.send(2).unwrap()), second).unwrap();
    s.enqueue(job(|_| {}), third).unwrap();
    let (run, p) = s.wait_next().unwrap();
    (run.run)(p);
    assert_eq!(rx.recv().unwrap(), 1);
    assert_eq!(s.snapshot().active, 1);
    let (run, p) = s.wait_next().unwrap();
    (run.run)(p);
    assert_eq!(rx2.recv().unwrap(), 2);
    assert_eq!(s.snapshot().queued, 0);
}
#[test]
fn test_queued_reservation_dispatches_before_later_submission_after_release() {
    let s = BoundedScheduler::new(config(1, 2, 1, 8)).unwrap();
    let a = s.try_reserve(0).unwrap();
    let b = s.try_reserve(0).unwrap();
    let (tx, rx) = mpsc::channel();
    let tx_a = tx.clone();
    s.enqueue(job(move |_| tx_a.send('A').unwrap()), a).unwrap();
    let tx_b = tx.clone();
    s.enqueue(job(move |_| tx_b.send('B').unwrap()), b).unwrap();

    let (run_a, permit_a) = s.wait_next().unwrap();
    (run_a.run)(permit_a);
    let c = s.try_reserve(0).unwrap();
    s.enqueue(job(move |_| tx.send('C').unwrap()), c).unwrap();
    assert_eq!((s.snapshot().active, s.snapshot().queued), (1, 1));

    let (run_b, permit_b) = s.wait_next().unwrap();
    (run_b.run)(permit_b);
    let (run_c, permit_c) = s.wait_next().unwrap();
    (run_c.run)(permit_c);
    assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec!['A', 'B', 'C']);
}
#[test]
fn test_enqueue_wakes_blocked_worker_and_close_drains() {
    let s = BoundedScheduler::new(config(1, 1, 1, 8)).unwrap();
    let worker = s.clone();
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tx.send(worker.wait_next().is_some()).unwrap();
        tx.send(worker.wait_next().is_none()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(10));
    s.enqueue(job(|_| {}), s.try_reserve(0).unwrap()).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    s.close_admission();
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    thread.join().unwrap();
}
#[test]
fn test_manual_clock_deadline_rejects_and_expired_accepted_job_dispatches_completion() {
    let s = BoundedScheduler::new(config(1, 1, 1, 8)).unwrap();
    let clock = ManualClock::new();
    let expired_context = clock.context(0);
    let scheduler_port: &dyn Scheduler = &s;
    assert_eq!(
        scheduler_port.try_reserve_until(0, Some(expired_context)).unwrap_err(),
        CapacityFailure::DeadlineExpired
    );
    let active = s.try_reserve(0).unwrap();
    let deadline_context = clock.context(10);
    let p = scheduler_port.try_reserve_until(0, Some(deadline_context)).unwrap();
    let ran = Arc::new(AtomicUsize::new(0));
    let count = ran.clone();
    let stops = Arc::new(std::sync::Mutex::new(Vec::new()));
    let run_stops = stops.clone();
    let stop_port = Arc::new(RecordingStop(stops.clone()));
    s.enqueue(
        ScheduledJob {
            run: Box::new(move |_| {
                assert!(run_stops.lock().unwrap().contains(&StopReason::Timeout));
                count.fetch_add(1, Ordering::SeqCst);
            }),
            stop: stop_port,
        },
        p,
    )
    .unwrap();
    drop(active);
    clock.advance_to(10);
    let (expired_job, permit) = s.try_next().unwrap();
    (expired_job.run)(permit);
    assert_eq!(stops.lock().unwrap().as_slice(), &[StopReason::Timeout]);
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!((s.snapshot().active, s.snapshot().queued), (0, 0));
}

#[test]
fn test_capacity_wait_rechecks_inherited_manual_clock_domain() {
    let s = BoundedScheduler::new(config(1, 0, 1, 8)).unwrap();
    let active = s.try_reserve(0).unwrap();
    let clock = ManualClock::new();
    let context = clock.context(10);
    let waiting = s.clone();
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tx.send(waiting.wait_for_capacity(0, Some(context))).unwrap();
    });
    let until = Instant::now() + Duration::from_secs(1);
    while s.snapshot().waiters == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    assert_eq!(s.snapshot().waiters, 1);
    clock.advance_to(10);
    assert!(!rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(s.snapshot().waiters, 0);
    drop(active);
    thread.join().unwrap();
}

#[test]
fn test_wait_next_requests_timeout_then_dispatches_completion_when_slot_opens() {
    let s = BoundedScheduler::new(config(1, 1, 1, 8)).unwrap();
    let active = s.try_reserve(0).unwrap();
    let clock = ManualClock::new();
    let scheduler_port: &dyn Scheduler = &s;
    let p = scheduler_port.try_reserve_until(0, Some(clock.context(10))).unwrap();
    let (stop_tx, stop_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    s.enqueue(
        ScheduledJob {
            run: Box::new(move |_| done_tx.send("terminal completion").unwrap()),
            stop: Arc::new(ChannelStop(stop_tx)),
        },
        p,
    )
    .unwrap();
    let worker = s.clone();
    let thread = std::thread::spawn(move || {
        let (job, permit) = worker.wait_next().unwrap();
        (job.run)(permit);
    });

    clock.advance_to(10);
    assert_eq!(
        stop_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        StopReason::Timeout
    );
    assert!(done_rx.try_recv().is_err());
    drop(active);
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "terminal completion"
    );
    thread.join().unwrap();
}

#[test]
fn test_concurrent_workers_wait_for_timeout_stop_callback_to_return() {
    let s = BoundedScheduler::new(config(2, 3, 2, 8)).unwrap();
    let active_a = s.try_reserve(0).unwrap();
    let active_b = s.try_reserve(0).unwrap();
    let clock = ManualClock::new();
    let scheduler_port: &dyn Scheduler = &s;
    let expired = scheduler_port.try_reserve_until(0, Some(clock.context(10))).unwrap();
    let later = s.try_reserve(0).unwrap();
    let last = s.try_reserve(0).unwrap();
    let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let (stop_tx, stop_rx) = mpsc::channel();
    let (run_tx, run_rx) = mpsc::channel();
    let stop_gate = gate.clone();
    s.enqueue(
        ScheduledJob {
            run: Box::new(|_| {}),
            stop: Arc::new(GatedStop {
                requested: stop_tx,
                gate: stop_gate,
            }),
        },
        expired,
    )
    .unwrap();
    let run_b = run_tx.clone();
    s.enqueue(job(move |_| run_b.send('B').unwrap()), later).unwrap();
    s.enqueue(job(move |_| run_tx.send('C').unwrap()), last).unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let worker = |scheduler: BoundedScheduler, started: Option<mpsc::Sender<()>>| {
        std::thread::spawn(move || {
            if let Some(started) = started {
                started.send(()).unwrap();
            }
            while let Some((job, permit)) = scheduler.wait_next() {
                (job.run)(permit);
            }
        })
    };
    let first = worker(s.clone(), None);
    clock.advance_to(10);
    assert_eq!(
        stop_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        StopReason::Timeout
    );
    drop(active_a);
    drop(active_b);
    let second = worker(s.clone(), Some(started_tx));
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(run_rx.recv_timeout(Duration::from_millis(30)).is_err());
    s.close_admission();

    let (released, changed) = &*gate;
    *released.lock().unwrap() = true;
    changed.notify_all();
    let mut ran = vec![run_rx.recv_timeout(Duration::from_secs(1)).unwrap()];
    ran.push(run_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    ran.sort_unstable();
    assert_eq!(ran, vec!['B', 'C']);
    first.join().unwrap();
    second.join().unwrap();
}
#[test]
fn test_close_after_reserve_returns_job_and_permit() {
    let s = BoundedScheduler::new(config(1, 1, 1, 9)).unwrap();
    let p = s.try_reserve(3).unwrap();
    s.close_admission();
    let f = s.enqueue(job(drop), p).unwrap_err();
    assert_eq!(f.reason, CapacityFailure::Closed);
    assert_eq!(s.snapshot().input_bytes, 3);
    drop(f);
    assert_eq!(s.snapshot().input_bytes, 0);
    assert_eq!(s.try_reserve(0).unwrap_err(), CapacityFailure::Closed);
}

#[test]
fn test_close_drains_already_accepted_active_and_queued_jobs() {
    let s = BoundedScheduler::new(config(1, 1, 1, 8)).unwrap();
    let a = s.try_reserve(0).unwrap();
    let b = s.try_reserve(0).unwrap();
    let (tx, rx) = mpsc::channel();
    let tx_a = tx.clone();
    s.enqueue(job(move |_| tx_a.send('A').unwrap()), a).unwrap();
    s.enqueue(job(move |_| tx.send('B').unwrap()), b).unwrap();
    s.close_admission();

    let (job_a, permit_a) = s.wait_next().unwrap();
    (job_a.run)(permit_a);
    let (job_b, permit_b) = s.wait_next().unwrap();
    (job_b.run)(permit_b);
    assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec!['A', 'B']);
    assert!(s.wait_next().is_none());
}

#[test]
fn test_blocking_waiters_share_the_configured_waiter_limit() {
    let scheduler = BoundedScheduler::new(config(1, 0, 1, 8)).unwrap();
    let permit = scheduler.try_reserve(0).unwrap();
    let waiting = scheduler.clone();
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tx.send(waiting.wait_for_capacity(0, None)).unwrap();
    });
    let until = Instant::now() + Duration::from_secs(1);
    while scheduler.snapshot().waiters == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    assert_eq!(scheduler.snapshot().waiters, 1);
    let expired_clock = ManualClock::new();
    let expired_context = expired_clock.context(0);
    assert!(!scheduler.wait_for_capacity(0, Some(expired_context)));
    drop(permit);
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    thread.join().unwrap();
    assert_eq!(scheduler.snapshot().waiters, 0);
}
