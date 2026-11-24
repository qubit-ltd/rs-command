// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Worker loop and completion helpers for retry timer messages.
use super::retry_timer_message::RetryTimerMessage;
use super::retry_timer_message::RetryTimerResult;

const RETRY_TIMER_DRAIN_BATCH: usize = 64;

/// Publishes a timer completion and wakes the latest registered retry task.
///
/// # Parameters
/// * `item`: Completed or cancelled timer message.
pub(super) fn complete_retry_timer(item: RetryTimerMessage) {
    let mut item = item;
    let state = std::sync::Arc::clone(&item.state);
    let waker = state.finish_with(RetryTimerResult::Ready, || drop(item.permit.take()));
    if let Some(waker) = waker {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
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
        if pending[index].state.is_cancelled() {
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
    run_retry_timer_with_before_wait(rx, || {});
}

/// Runs the timer worker with a hook immediately before it waits for a message.
///
/// This keeps shutdown behavior deterministic in tests while the production
/// worker supplies a no-op hook.
pub(super) fn run_retry_timer_with_before_wait(
    rx: std::sync::mpsc::Receiver<RetryTimerMessage>,
    mut before_wait: impl FnMut(),
) {
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
                    process_retry_timers(&mut pending, std::time::Instant::now());
                    drop(pending);
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
        before_wait();
        match rx.recv_timeout(wait) {
            Ok(item) => pending.push(item),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                process_retry_timers(&mut pending, std::time::Instant::now());
                drop(pending);
                return;
            }
        }
    }
}
