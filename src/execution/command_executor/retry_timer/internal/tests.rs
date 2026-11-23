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
use super::retry_timer_worker::complete_retry_timer;
use super::retry_timer_worker::process_retry_timers;

fn reject_retry_schedule(_: RetryTimerMessage) -> Result<(), RetryTimerFailure> {
    Err(RetryTimerFailure::Disconnected)
}

fn retry_timer_message(due: std::time::Instant) -> RetryTimerMessage {
    RetryTimerMessage {
        due,
        waker: Arc::new(std::sync::Mutex::new(None)),
        ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        permit: None,
    }
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

    assert!(delay
        .poll_with_schedule(&mut first_context, |item| {
            scheduled = Some(item);
            Ok(())
        })
        .is_pending());
    assert!(delay
        .poll_with_schedule(&mut second_context, reject_retry_schedule)
        .is_pending());
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
    assert!(delay
        .poll_with_schedule(&mut cx, |item| {
            scheduled = Some(item);
            Ok(())
        })
        .is_pending());
    let item = scheduled.take().expect("retry poll must construct timer message");
    let cancelled = Arc::clone(&item.cancelled);
    let waker_slot = Arc::clone(&item.waker);
    drop(delay);
    assert!(cancelled.load(Ordering::Acquire));
    assert!(waker_slot.lock().unwrap().is_none());
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
    let mut completed = rx.try_recv().unwrap();
    let callback = Arc::new(RescheduleOnWake {
        timer,
        result: std::sync::Mutex::new(None),
    });
    completed.waker = Arc::new(std::sync::Mutex::new(Some(Waker::from(callback.clone()))));

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
    pending[0].cancelled.store(true, Ordering::Release);
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
    let mut expired =
        retry_timer_message(std::time::Instant::now() - std::time::Duration::from_millis(1));
    expired.waker = Arc::new(std::sync::Mutex::new(Some(Waker::from(wakes.clone()))));
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
    assert!(delay
        .poll_with_schedule(&mut context, |item| {
            scheduled = Some(item);
            Ok(())
        })
        .is_pending());

    let slot = Arc::clone(&delay.waker);
    let poison = std::thread::spawn(move || {
        let _guard = slot.lock().unwrap();
        panic!("simulate a timer worker panic while holding its waker slot");
    })
    .join();
    assert!(poison.is_err());
    assert!(delay
        .poll_with_schedule(&mut context, reject_retry_schedule)
        .is_pending());
    drop(delay);
    complete_retry_timer(scheduled.take().expect("first poll must schedule a message"));

    let waker_slot = Arc::new(std::sync::Mutex::new(None));
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let poison = Arc::clone(&waker_slot);
    let join = std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("simulate a timer worker panic while publishing completion");
    })
    .join();
    assert!(join.is_err());
    complete_retry_timer(RetryTimerMessage {
        due: std::time::Instant::now(),
        waker: waker_slot,
        ready: Arc::clone(&ready),
        cancelled,
        permit: None,
    });
    assert!(ready.load(std::sync::atomic::Ordering::Acquire));
}
