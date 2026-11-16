use qubit_command::execution::scheduler::BoundedScheduler;
use qubit_command::execution::{
    CapacityFailure, CommandExecutionConfig, ScheduledJob, Scheduler, StopPort,
};
use qubit_operation::StopReason;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    task::{Wake, Waker},
    time::{Duration, Instant},
};
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
fn capacity_bounds_and_raii_release() {
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
fn zero_queue_only_admits_active_slots() {
    let s = BoundedScheduler::new(config(1, 0, 0, 9)).unwrap();
    let p = s.try_reserve(9).unwrap();
    assert_eq!(s.try_reserve(0).unwrap_err(), CapacityFailure::ActiveFull);
    drop(p);
    assert!(s.try_reserve(0).is_ok());
}
#[test]
fn input_bytes_precede_slots_and_are_in_capacity_wait_predicate() {
    let s = BoundedScheduler::new(config(1, 2, 2, 2)).unwrap();
    let p = s.try_reserve(2).unwrap();
    assert_eq!(
        s.try_reserve(1).unwrap_err(),
        CapacityFailure::InputBytesExceeded
    );
    let waiting = s.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        tx.send(waiting.wait_for_capacity(1, Some(Instant::now() + Duration::from_secs(2))))
            .unwrap()
    });
    drop(p);
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
}
#[test]
fn waiter_registration_cancel_and_release_are_race_safe() {
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
fn foreign_permit_with_colliding_token_is_rejected_and_returned() {
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
    assert_eq!(
        (a_before.active, a_before.queued, a_before.input_bytes),
        (1, 1, 3)
    );
    assert_eq!(
        (b_before.active, b_before.queued, b_before.input_bytes),
        (1, 1, 7)
    );
    drop(failure);
    let a_after = a.snapshot();
    let b_after = b.snapshot();
    assert_eq!(
        (a_after.active, a_after.queued, a_after.input_bytes),
        (1, 1, 3)
    );
    assert_eq!(
        (b_after.active, b_after.queued, b_after.input_bytes),
        (1, 0, 3)
    );
    drop(own_queued);
    drop(own_active);
    drop(origin_active);
}
#[test]
fn multi_job_queue_promotes_only_after_active_release() {
    let s = BoundedScheduler::new(config(1, 2, 1, 8)).unwrap();
    let first = s.try_reserve(0).unwrap();
    let second = s.try_reserve(0).unwrap();
    let third = s.try_reserve(0).unwrap();
    let (tx, rx) = mpsc::channel();
    s.enqueue(job(move |_| tx.send(1).unwrap()), first).unwrap();
    let (tx, rx2) = mpsc::channel();
    s.enqueue(job(move |_| tx.send(2).unwrap()), second)
        .unwrap();
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
fn queued_reservation_dispatches_before_later_submission_after_release() {
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
fn enqueue_wakes_blocked_worker_and_close_drains() {
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
fn fixed_deadline_rejects_and_expired_accepted_job_dispatches_terminal_completion() {
    let s = BoundedScheduler::new(config(1, 1, 1, 8)).unwrap();
    let scheduler_port: &dyn Scheduler = &s;
    assert_eq!(
        scheduler_port
            .try_reserve_until(0, Some(Instant::now()))
            .unwrap_err(),
        CapacityFailure::DeadlineExpired
    );
    let active = s.try_reserve(0).unwrap();
    let p = scheduler_port
        .try_reserve_until(0, Some(Instant::now() + Duration::from_millis(20)))
        .unwrap();
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
    std::thread::sleep(Duration::from_millis(30));
    let (expired_job, permit) = s.try_next().unwrap();
    (expired_job.run)(permit);
    assert_eq!(stops.lock().unwrap().as_slice(), &[StopReason::Timeout]);
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!((s.snapshot().active, s.snapshot().queued), (0, 0));
}

#[test]
fn wait_next_requests_timeout_then_dispatches_completion_when_slot_opens() {
    let s = BoundedScheduler::new(config(1, 1, 1, 8)).unwrap();
    let active = s.try_reserve(0).unwrap();
    let scheduler_port: &dyn Scheduler = &s;
    let p = scheduler_port
        .try_reserve_until(0, Some(Instant::now() + Duration::from_millis(20)))
        .unwrap();
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
fn concurrent_workers_wait_for_timeout_stop_callback_to_return() {
    let s = BoundedScheduler::new(config(1, 1, 2, 8)).unwrap();
    let active = s.try_reserve(0).unwrap();
    let scheduler_port: &dyn Scheduler = &s;
    let permit = scheduler_port
        .try_reserve_until(0, Some(Instant::now() + Duration::from_millis(20)))
        .unwrap();
    let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let (stop_tx, stop_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let stop_gate = gate.clone();
    s.enqueue(
        ScheduledJob {
            run: Box::new(move |_| done_tx.send(()).unwrap()),
            stop: Arc::new(GatedStop {
                requested: stop_tx,
                gate: stop_gate,
            }),
        },
        permit,
    )
    .unwrap();

    let (dispatch_tx, dispatch_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let worker = |id,
                  scheduler: BoundedScheduler,
                  dispatch: mpsc::Sender<usize>,
                  started: Option<mpsc::Sender<()>>| {
        std::thread::spawn(move || {
            if let Some(started) = started {
                started.send(()).unwrap();
            }
            if let Some((job, permit)) = scheduler.wait_next() {
                dispatch.send(id).unwrap();
                (job.run)(permit);
            }
        })
    };
    let first = worker(1, s.clone(), dispatch_tx.clone(), None);
    assert_eq!(
        stop_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        StopReason::Timeout
    );
    drop(active);
    let second = worker(2, s.clone(), dispatch_tx, Some(started_tx));
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(dispatch_rx.recv_timeout(Duration::from_millis(30)).is_err());

    let (released, changed) = &*gate;
    *released.lock().unwrap() = true;
    changed.notify_all();
    dispatch_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    s.close_admission();
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    first.join().unwrap();
    second.join().unwrap();
}
#[test]
fn close_after_reserve_returns_job_and_permit() {
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
fn close_drains_already_accepted_active_and_queued_jobs() {
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
fn blocking_waiters_share_the_configured_waiter_limit() {
    let scheduler = BoundedScheduler::new(config(1, 0, 1, 8)).unwrap();
    let permit = scheduler.try_reserve(0).unwrap();
    let waiting = scheduler.clone();
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tx.send(waiting.wait_for_capacity(0, Some(Instant::now() + Duration::from_secs(2))))
            .unwrap();
    });
    let until = Instant::now() + Duration::from_secs(1);
    while scheduler.snapshot().waiters == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    assert_eq!(scheduler.snapshot().waiters, 1);
    assert!(!scheduler.wait_for_capacity(0, Some(Instant::now())));
    drop(permit);
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    thread.join().unwrap();
    assert_eq!(scheduler.snapshot().waiters, 0);
}
