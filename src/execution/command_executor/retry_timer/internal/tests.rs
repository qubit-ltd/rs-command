// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use super::retry_delay::RetryDelay;
use super::retry_timer::RetryTimer;
use super::retry_timer_failure::RetryTimerFailure;
use super::retry_timer_message::RetryTimerMessage;
use super::retry_timer_result::RetryTimerResult;
use super::retry_timer_state::RetryTimerState;
use super::retry_timer_worker::complete_retry_timer;
use super::retry_timer_worker::process_retry_timers;
use super::retry_timer_worker::run_retry_timer;
use super::retry_timer_worker::run_retry_timer_with_before_wait;

fn reject_retry_schedule(_: RetryTimerMessage) -> Result<(), RetryTimerFailure> {
    Err(RetryTimerFailure::Disconnected)
}

fn retry_timer_message(due: std::time::Instant) -> RetryTimerMessage {
    RetryTimerMessage {
        due,
        state: Arc::new(RetryTimerState::new()),
        permit: None,
        accepted: false,
    }
}

fn schedule_timer(timer: &RetryTimer, due: std::time::Instant) -> Arc<RetryTimerState> {
    let state = Arc::new(RetryTimerState::new());
    timer
        .schedule(RetryTimerMessage {
            due,
            state: Arc::clone(&state),
            permit: None,
            accepted: false,
        })
        .unwrap();
    state
}

struct CountWake(AtomicUsize);

impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct RescheduleOnWake {
    timer: RetryTimer,
    result: std::sync::Mutex<Option<Result<(), RetryTimerFailure>>>,
}

impl Wake for RescheduleOnWake {
    fn wake(self: Arc<Self>) {
        let result = self.timer.schedule(retry_timer_message(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ));
        *self.result.lock().unwrap() = Some(result);
    }
}

struct NotifyWake(std::sync::mpsc::Sender<()>);

impl Wake for NotifyWake {
    fn wake(self: Arc<Self>) {
        let _ = self.0.send(());
    }
}

struct PanicWake;

impl Wake for PanicWake {
    fn wake(self: Arc<Self>) {
        panic!("controlled retry timer waker panic");
    }
}

#[test]
fn test_retry_timer_wakes_the_most_recently_registered_waker() {
    let mut delay = Box::pin(RetryDelay::new());
    let first = Arc::new(CountWake(AtomicUsize::new(0)));
    let second = Arc::new(CountWake(AtomicUsize::new(0)));
    let first_waker = Waker::from(first.clone());
    let second_waker = Waker::from(second.clone());
    let mut first_context = Context::from_waker(&first_waker);
    let mut second_context = Context::from_waker(&second_waker);
    let mut scheduled = None;

    assert!(
        delay
            .poll_with_schedule(&mut first_context, |item| {
                scheduled = Some(item);
                Ok(())
            })
            .is_pending()
    );
    assert!(
        delay
            .poll_with_schedule(&mut second_context, reject_retry_schedule)
            .is_pending()
    );
    complete_retry_timer(scheduled.take().expect("first poll must schedule a message"));

    assert_eq!(first.0.load(Ordering::SeqCst), 0);
    assert_eq!(second.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        delay.poll_with_schedule(&mut second_context, reject_retry_schedule),
        Poll::Ready(Ok(()))
    ));
}

#[test]
fn test_retry_delay_schedule_and_completion_boundaries() {
    let mut delay = Box::pin(RetryDelay::new());
    let wake_count = Arc::new(CountWake(AtomicUsize::new(0)));
    let waker = Waker::from(wake_count.clone());
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(
        delay.poll_with_schedule(&mut cx, reject_retry_schedule),
        Poll::Ready(Err(RetryTimerFailure::Disconnected))
    ));
    assert!(!delay.scheduled);

    let mut scheduled = None;
    assert!(
        delay
            .poll_with_schedule(&mut cx, |item| {
                scheduled = Some(item);
                Ok(())
            })
            .is_pending()
    );
    let item = scheduled.take().expect("retry poll must construct timer message");
    let state = Arc::clone(&item.state);
    drop(delay);
    assert!(state.is_cancelled());
    assert!(state.inner.lock().unwrap().0.is_none());
    complete_retry_timer(item);
    assert_eq!(wake_count.0.load(Ordering::SeqCst), 0);
}

#[test]
fn test_retry_timer_quota_covers_queued_and_pending_messages() {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let timer = RetryTimer::new(tx, 2);
    let mut pending = Vec::new();
    timer
        .schedule(retry_timer_message(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ))
        .unwrap();
    pending.push(rx.try_recv().unwrap());
    timer
        .schedule(retry_timer_message(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ))
        .unwrap();

    assert_eq!(timer.in_flight(), 2);
    assert_eq!(
        timer.schedule(retry_timer_message(std::time::Instant::now())),
        Err(RetryTimerFailure::Full)
    );

    complete_retry_timer(pending.pop().unwrap());
    assert_eq!(timer.in_flight(), 1);
    timer
        .schedule(retry_timer_message(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ))
        .unwrap();
    assert_eq!(timer.in_flight(), 2);
}

#[test]
fn test_retry_timer_releases_quota_before_waking_registered_task() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let timer = RetryTimer::new(tx, 1);
    timer
        .schedule(retry_timer_message(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ))
        .unwrap();
    let completed = rx.try_recv().unwrap();
    let callback = Arc::new(RescheduleOnWake {
        timer,
        result: std::sync::Mutex::new(None),
    });
    completed.state.inner.lock().unwrap().0 = Some(Waker::from(callback.clone()));

    complete_retry_timer(completed);

    assert_eq!(*callback.result.lock().unwrap(), Some(Ok(())));
    assert_eq!(callback.timer.in_flight(), 1);
}

#[test]
fn test_retry_timer_returns_quota_when_send_fails_or_pending_is_cancelled() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let timer = RetryTimer::new(tx, 1);
    timer
        .schedule(retry_timer_message(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ))
        .unwrap();
    let mut pending = vec![rx.try_recv().unwrap()];
    pending[0].state.cancel();
    process_retry_timers(&mut pending, std::time::Instant::now());
    assert_eq!(timer.in_flight(), 0);

    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    drop(rx);
    let disconnected = RetryTimer::new(tx, 1);
    assert_eq!(
        disconnected.schedule(retry_timer_message(std::time::Instant::now())),
        Err(RetryTimerFailure::Disconnected)
    );
    assert_eq!(disconnected.in_flight(), 0);
}

#[test]
fn test_retry_timer_processes_expired_pending_while_queue_has_messages() {
    let (tx, rx) = std::sync::mpsc::sync_channel(64);
    let timer = RetryTimer::new(tx, 64);
    let wakes = Arc::new(CountWake(AtomicUsize::new(0)));
    let expired = retry_timer_message(std::time::Instant::now() - std::time::Duration::from_millis(1));
    expired.state.inner.lock().unwrap().0 = Some(Waker::from(wakes.clone()));
    let mut pending = vec![expired];
    for _ in 0..64 {
        timer
            .schedule(retry_timer_message(
                std::time::Instant::now() + std::time::Duration::from_secs(30),
            ))
            .unwrap();
    }

    process_retry_timers(&mut pending, std::time::Instant::now());

    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(pending.is_empty());
    assert_eq!(timer.in_flight(), 64);
    assert_eq!(rx.try_iter().count(), 64);
}

#[test]
fn test_retry_delay_production_timer_wakes_and_completes() {
    let (tx, rx) = std::sync::mpsc::channel();
    let waker = Waker::from(Arc::new(NotifyWake(tx)));
    let mut cx = Context::from_waker(&waker);
    let mut delay = Box::pin(RetryDelay::new());

    assert!(delay.as_mut().poll(&mut cx).is_pending());
    rx.recv_timeout(std::time::Duration::from_secs(2))
        .expect("production retry timer must wake the registered task");
    assert!(matches!(delay.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
}

#[test]
fn test_retry_timer_recovers_poisoned_waker_slots() {
    let mut delay = Box::pin(RetryDelay::new());
    let waker = Waker::noop().clone();
    let mut context = Context::from_waker(&waker);
    let mut scheduled = None;
    assert!(
        delay
            .poll_with_schedule(&mut context, |item| {
                scheduled = Some(item);
                Ok(())
            })
            .is_pending()
    );

    let slot = Arc::clone(&delay.state);
    let poison = std::thread::spawn(move || {
        let _guard = slot.inner.lock().unwrap();
        panic!("simulate a timer worker panic while holding its waker slot");
    })
    .join();
    assert!(poison.is_err());
    assert!(
        delay
            .poll_with_schedule(&mut context, reject_retry_schedule)
            .is_pending()
    );
    drop(delay);
    complete_retry_timer(scheduled.take().expect("first poll must schedule a message"));

    let state = Arc::new(RetryTimerState::new());
    let poison = Arc::clone(&state);
    let join = std::thread::spawn(move || {
        let _guard = poison.inner.lock().unwrap();
        panic!("simulate a timer worker panic while publishing completion");
    })
    .join();
    assert!(join.is_err());
    complete_retry_timer(RetryTimerMessage {
        due: std::time::Instant::now(),
        state: Arc::clone(&state),
        permit: None,
        accepted: true,
    });
    assert_eq!(state.poll(&Waker::noop().clone()), RetryTimerResult::Ready);
}

#[test]
fn test_retry_timer_isolates_a_panicking_waker_and_completes_later_items() {
    let first = retry_timer_message(std::time::Instant::now());
    first.state.inner.lock().unwrap().0 = Some(Waker::from(Arc::new(PanicWake)));
    let (tx, rx) = std::sync::mpsc::channel();
    let second = retry_timer_message(std::time::Instant::now());
    second.state.inner.lock().unwrap().0 = Some(Waker::from(Arc::new(NotifyWake(tx))));
    let second_state = Arc::clone(&second.state);

    let worker = std::thread::spawn(move || {
        process_retry_timers(&mut vec![first, second], std::time::Instant::now());
    });

    worker
        .join()
        .expect("one panicking waker must not terminate the worker");
    rx.recv_timeout(std::time::Duration::from_secs(1))
        .expect("the later timer waiter must still be woken");
    assert_eq!(second_state.poll(&Waker::noop().clone()), RetryTimerResult::Ready);
}

#[test]
fn test_retry_timer_worker_panic_disconnects_queued_and_pending_waiters() {
    let (tx, rx) = std::sync::mpsc::sync_channel(2);
    let timer = RetryTimer::new(tx, 2);
    let wake = Waker::noop().clone();
    let mut cx = Context::from_waker(&wake);
    let mut pending_delay = Box::pin(RetryDelay::new());
    let mut queued_delay = Box::pin(RetryDelay::new());

    assert!(
        pending_delay
            .poll_with_schedule(&mut cx, |item| timer.schedule(item))
            .is_pending()
    );
    let pending = rx
        .try_recv()
        .expect("first retry timer should be pending in the worker");
    assert!(
        queued_delay
            .poll_with_schedule(&mut cx, |item| timer.schedule(item))
            .is_pending()
    );
    assert_eq!(timer.in_flight(), 2);

    let worker = std::thread::spawn(move || {
        let _pending = vec![pending];
        let _receiver = rx;
        panic!("controlled timer worker failure");
    });
    assert!(worker.join().is_err());

    assert_eq!(timer.in_flight(), 0);
    assert!(matches!(
        pending_delay.as_mut().poll(&mut cx),
        Poll::Ready(Err(RetryTimerFailure::Disconnected))
    ));
    assert!(matches!(
        queued_delay.as_mut().poll(&mut cx),
        Poll::Ready(Err(RetryTimerFailure::Disconnected))
    ));
}

#[test]
fn test_retry_timer_shutdown_before_worker_start_disconnects_queued_messages() {
    let (tx, rx) = std::sync::mpsc::sync_channel(2);
    let timer = RetryTimer::new(tx, 2);
    let in_flight = timer.in_flight_counter();
    let first = schedule_timer(&timer, std::time::Instant::now() - std::time::Duration::from_millis(1));
    let second = schedule_timer(&timer, std::time::Instant::now() + std::time::Duration::from_secs(60));
    drop(timer);

    let worker = std::thread::spawn(move || run_retry_timer(rx));
    worker.join().expect("closed timer worker should exit normally");

    assert_eq!(first.poll(&Waker::noop().clone()), RetryTimerResult::Ready);
    assert_eq!(second.poll(&Waker::noop().clone()), RetryTimerResult::Disconnected);
    assert_eq!(in_flight.load(Ordering::Acquire), 0);
}

#[test]
fn test_retry_timer_shutdown_while_waiting_disconnects_pending_messages() {
    let (tx, rx) = std::sync::mpsc::sync_channel(2);
    let timer = RetryTimer::new(tx, 2);
    let in_flight = timer.in_flight_counter();
    let first = schedule_timer(&timer, std::time::Instant::now() - std::time::Duration::from_millis(1));
    let second = schedule_timer(&timer, std::time::Instant::now() + std::time::Duration::from_secs(60));
    let mut sender = Some(timer);

    let worker = std::thread::spawn(move || {
        run_retry_timer_with_before_wait(rx, move || drop(sender.take()));
    });
    worker
        .join()
        .expect("worker should exit after the last sender closes during its wait");

    assert_eq!(first.poll(&Waker::noop().clone()), RetryTimerResult::Ready);
    assert_eq!(second.poll(&Waker::noop().clone()), RetryTimerResult::Disconnected);
    assert_eq!(in_flight.load(Ordering::Acquire), 0);
}
