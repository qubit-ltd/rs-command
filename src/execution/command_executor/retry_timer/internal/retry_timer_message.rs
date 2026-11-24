// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Message state transferred from retry tasks to the timer worker.
use super::retry_timer_permit::RetryTimerPermit;

/// The shared completion state observed by a timer message and its future.
pub(super) struct RetryTimerState {
    /// The latest task waker and the single terminal result.
    pub(super) inner: std::sync::Mutex<RetryTimerStateInner>,
}

/// Mutable state protected while publishing completion or registering a waker.
pub(super) struct RetryTimerStateInner {
    /// The most recently registered task waker.
    pub(super) waker: Option<std::task::Waker>,
    /// The result visible to the future, if a terminal transition occurred.
    pub(super) result: RetryTimerResult,
}

/// A retry timer's shared lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetryTimerResult {
    /// The worker has not completed or disconnected the timer message.
    Pending,
    /// The deadline elapsed.
    Ready,
    /// The worker discarded an accepted message.
    Disconnected,
    /// The retry future was dropped.
    Cancelled,
}

impl RetryTimerState {
    /// Creates a timer state with no registered waker.
    pub(super) fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(RetryTimerStateInner {
                waker: None,
                result: RetryTimerResult::Pending,
            }),
        }
    }

    /// Takes the current result and registers the latest waker when pending.
    pub(super) fn poll(&self, waker: &std::task::Waker) -> RetryTimerResult {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.result == RetryTimerResult::Pending
            && !inner
                .waker
                .as_ref()
                .is_some_and(|registered| registered.will_wake(waker))
        {
            inner.waker = Some(waker.clone());
        }
        inner.result
    }

    /// Marks an uncompleted timer as cancelled and discards its waker.
    pub(super) fn cancel(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.result == RetryTimerResult::Pending {
            inner.result = RetryTimerResult::Cancelled;
        }
        inner.waker = None;
    }

    /// Reports whether the future has been dropped.
    pub(super) fn is_cancelled(&self) -> bool {
        self.inner.lock().unwrap_or_else(|error| error.into_inner()).result == RetryTimerResult::Cancelled
    }

    /// Moves a pending timer to a terminal state and releases its quota before
    /// making that state observable to another poll.
    pub(super) fn finish_with(
        &self,
        result: RetryTimerResult,
        release_quota: impl FnOnce(),
    ) -> Option<std::task::Waker> {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let waker = if inner.result == RetryTimerResult::Pending {
            inner.result = result;
            inner.waker.take()
        } else {
            None
        };
        release_quota();
        waker
    }
}

/// Carries one retry deadline and its task notification state.
pub(super) struct RetryTimerMessage {
    /// The instant when the retry task becomes ready.
    pub(super) due: std::time::Instant,
    /// State shared with the retry future.
    pub(super) state: std::sync::Arc<RetryTimerState>,
    /// The shared quota slot held by this queued or pending message.
    pub(super) permit: Option<RetryTimerPermit>,
    /// Whether the timer accepted this message into its queue.
    pub(super) accepted: bool,
}

impl Drop for RetryTimerMessage {
    /// Resolves an accepted message if its worker drops it without completion.
    fn drop(&mut self) {
        let state = std::sync::Arc::clone(&self.state);
        let waker = if self.accepted {
            state.finish_with(RetryTimerResult::Disconnected, || drop(self.permit.take()))
        } else {
            drop(self.permit.take());
            None
        };
        if let Some(waker) = waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
        }
    }
}
