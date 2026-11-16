use qubit_command::execution::scheduler::BoundedScheduler;
use qubit_command::execution::{
    CapacityFailure, CommandExecutionConfig, ScheduledJob, Scheduler, StopPort,
};
use qubit_operation::StopReason;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Stop;
impl StopPort for Stop {
    fn request_stop(&self, _: StopReason) {}
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
fn job() -> ScheduledJob {
    ScheduledJob {
        run: Box::new(|| {}),
        stop: Arc::new(Stop),
    }
}

#[test]
fn capacity_bounds_and_reservation_drop_release_only_its_own_counts() {
    let s = BoundedScheduler::new(config(4, 64, 64, 1024)).unwrap();
    let mut permits = Vec::new();
    for _ in 0..68 {
        permits.push(s.try_reserve(1).unwrap());
    }
    let snap = s.snapshot();
    assert_eq!((snap.active, snap.queued, snap.input_bytes), (4, 64, 68));
    assert_eq!(s.try_reserve(1).unwrap_err(), CapacityFailure::QueueFull);
    drop(permits.remove(0));
    let snap = s.snapshot();
    assert_eq!((snap.active, snap.queued, snap.input_bytes), (3, 64, 67));
    assert!(s.try_reserve(1).is_ok());
}
#[test]
fn zero_queue_only_admits_immediate_active_slots() {
    let s = BoundedScheduler::new(config(1, 0, 0, 9)).unwrap();
    let permit = s.try_reserve(9).unwrap();
    assert_eq!(s.try_reserve(0).unwrap_err(), CapacityFailure::ActiveFull);
    drop(permit);
    assert!(s.try_reserve(0).is_ok());
}
#[test]
fn byte_budget_is_checked_before_capacity_and_failure_returns_no_accounting() {
    let s = BoundedScheduler::new(config(1, 4, 2, 3)).unwrap();
    let p = s.try_reserve(2).unwrap();
    assert_eq!(
        s.try_reserve(2).unwrap_err(),
        CapacityFailure::InputBytesExceeded
    );
    assert_eq!(s.snapshot().input_bytes, 2);
    drop(p);
    assert_eq!(s.snapshot().input_bytes, 0);
}
#[test]
fn waiter_limit_cancel_and_release_wake_are_bounded() {
    let s = BoundedScheduler::new(config(1, 0, 2, 10)).unwrap();
    let ids = [
        s.register_waiter(None).unwrap(),
        s.register_waiter(None).unwrap(),
    ];
    assert_eq!(
        s.register_waiter(None).unwrap_err(),
        CapacityFailure::QueueFull
    );
    s.cancel_waiter(ids[0]);
    assert_eq!(s.snapshot().waiters, 1);
    drop(s.try_reserve(0).unwrap());
    assert!(s.wait_for_capacity(Some(
        std::time::Instant::now() + std::time::Duration::from_millis(5)
    )));
}
#[test]
fn close_after_reserve_returns_enqueue_inputs_and_permit() {
    let s = BoundedScheduler::new(config(1, 1, 1, 9)).unwrap();
    let p = s.try_reserve(3).unwrap();
    s.close_admission();
    let failure = s.enqueue(job(), p).unwrap_err();
    assert_eq!(failure.reason, CapacityFailure::Closed);
    assert_eq!(s.snapshot().input_bytes, 3);
    drop(failure);
    assert_eq!(s.snapshot().input_bytes, 0);
    assert_eq!(s.try_reserve(0).unwrap_err(), CapacityFailure::Closed);
}
#[test]
fn job_keeps_active_permit_until_run_finishes() {
    let s = BoundedScheduler::new(config(1, 1, 1, 9)).unwrap();
    let ran = Arc::new(AtomicUsize::new(0));
    let count = ran.clone();
    let p = s.try_reserve(0).unwrap();
    let j = ScheduledJob {
        run: Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }),
        stop: Arc::new(Stop),
    };
    s.enqueue(j, p).unwrap();
    let j = s.take_next().unwrap();
    assert_eq!(s.snapshot().active, 1);
    (j.run)();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!(s.snapshot().active, 0);
}
