// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Message state transferred from retry tasks to the timer worker.
use super::retry_timer_permit::RetryTimerPermit;
use super::retry_timer_result::RetryTimerResult;
use super::retry_timer_state::RetryTimerState;

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
    ///
    /// # Parameters
    /// * `self`: Timer message being discarded.
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
