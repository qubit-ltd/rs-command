// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Worker loop and completion helpers for retry timer messages.
use super::retry_timer_message::RetryTimerMessage;

const RETRY_TIMER_DRAIN_BATCH: usize = 64;

/// Publishes a timer completion and wakes the latest registered retry task.
///
/// # Parameters
/// * `item`: Completed or cancelled timer message.
pub(super) fn complete_retry_timer(item: RetryTimerMessage) {
    if item.cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    drop(item.permit);
    item.ready.store(true, std::sync::atomic::Ordering::Release);
    let waker = item.waker.lock().unwrap_or_else(|error| error.into_inner()).take();
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// Removes cancelled messages and completes messages whose deadlines elapsed.
///
/// # Parameters
/// * `pending`: Worker-owned messages waiting for completion.
/// * `now`: Current instant used to identify elapsed deadlines.
pub(super) fn process_retry_timers(pending: &mut Vec<RetryTimerMessage>, now: std::time::Instant) {
    let mut index = 0;
    while index < pending.len() {
        if pending[index].cancelled.load(std::sync::atomic::Ordering::Acquire) {
            pending.swap_remove(index);
        } else if pending[index].due <= now {
            complete_retry_timer(pending.swap_remove(index));
        } else {
            index += 1;
        }
    }
}

/// Receives timer requests in bounded batches and processes due work between
/// them.
///
/// # Parameters
/// * `rx`: Receiver supplying scheduled timer messages.
pub(super) fn run_retry_timer(rx: std::sync::mpsc::Receiver<RetryTimerMessage>) {
    let mut pending = Vec::<RetryTimerMessage>::new();
    loop {
        let mut received = 0;
        while received < RETRY_TIMER_DRAIN_BATCH {
            match rx.try_recv() {
                Ok(item) => {
                    pending.push(item);
                    received += 1;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    for item in pending.drain(..) {
                        complete_retry_timer(item);
                    }
                    return;
                }
            }
        }
        process_retry_timers(&mut pending, std::time::Instant::now());
        if received == RETRY_TIMER_DRAIN_BATCH {
            continue;
        }
        let wait = pending
            .iter()
            .map(|item| item.due.saturating_duration_since(std::time::Instant::now()))
            .min()
            .unwrap_or(std::time::Duration::from_secs(30));
        match rx.recv_timeout(wait) {
            Ok(item) => pending.push(item),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                for item in pending.drain(..) {
                    complete_retry_timer(item);
                }
                return;
            }
        }
    }
}
