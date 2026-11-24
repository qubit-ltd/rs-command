// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared result and waker state for one retry timer request.
use super::retry_timer_result::RetryTimerResult;

/// The shared completion state observed by a timer message and its future.
pub(super) struct RetryTimerState {
    /// The latest task waker and the single terminal result.
    pub(super) inner: std::sync::Mutex<(Option<std::task::Waker>, RetryTimerResult)>,
}

impl RetryTimerState {
    /// Creates a timer state with no registered waker.
    ///
    /// # Returns
    /// The new pending timer state.
    pub(super) fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new((None, RetryTimerResult::Pending)),
        }
    }

    /// Registers the latest waker and reads the current timer result.
    ///
    /// # Parameters
    /// * `waker`: Task waker to use if the timer is still pending.
    ///
    /// # Returns
    /// The current timer result.
    pub(super) fn poll(&self, waker: &std::task::Waker) -> RetryTimerResult {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.1 == RetryTimerResult::Pending
            && !inner.0.as_ref().is_some_and(|registered| registered.will_wake(waker))
        {
            inner.0 = Some(waker.clone());
        }
        inner.1
    }

    /// Marks an uncompleted timer as cancelled and discards its waker.
    ///
    /// # Parameters
    /// * `self`: Timer state to mark cancelled.
    pub(super) fn cancel(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.1 == RetryTimerResult::Pending {
            inner.1 = RetryTimerResult::Cancelled;
        }
        inner.0 = None;
    }

    /// Reports whether the retry future has been dropped.
    ///
    /// # Parameters
    /// * `self`: Timer state to inspect.
    ///
    /// # Returns
    /// `true` if the timer state is cancelled.
    pub(super) fn is_cancelled(&self) -> bool {
        self.inner.lock().unwrap_or_else(|error| error.into_inner()).1 == RetryTimerResult::Cancelled
    }

    /// Completes a pending timer and releases quota before the result becomes
    /// observable to another poll.
    ///
    /// # Parameters
    /// * `result`: Terminal result to publish.
    /// * `release_quota`: Callback that releases the timer's in-flight slot.
    ///
    /// # Returns
    /// The registered waker when this call completed a pending timer.
    pub(super) fn finish_with(
        &self,
        result: RetryTimerResult,
        release_quota: impl FnOnce(),
    ) -> Option<std::task::Waker> {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let waker = if inner.1 == RetryTimerResult::Pending {
            inner.1 = result;
            inner.0.take()
        } else {
            None
        };
        release_quota();
        waker
    }
}
