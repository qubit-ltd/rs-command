// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Future that becomes ready after a bounded retry delay.
use super::retry_timer::retry_timer;
use super::retry_timer_failure::RetryTimerFailure;
use super::retry_timer_message::RetryTimerMessage;
use super::retry_timer_message::RetryTimerResult;
use super::retry_timer_message::RetryTimerState;

/// Waits for one scheduled retry delay and tracks its latest task waker.
pub struct RetryDelay {
    /// Timer result and latest waker shared with the worker message.
    pub(super) state: std::sync::Arc<RetryTimerState>,
    /// Whether a timer message has been queued.
    pub(super) scheduled: bool,
}

impl RetryDelay {
    /// Creates a new retry delay future.
    pub fn new() -> Self {
        Self {
            state: std::sync::Arc::new(RetryTimerState::new()),
            scheduled: false,
        }
    }

    /// Polls the delay while delegating message scheduling.
    ///
    /// # Parameters
    /// * `cx`: Task context used to register the latest waker.
    /// * `schedule`: Callback that queues the retry timer message.
    pub(super) fn poll_with_schedule(
        &mut self,
        cx: &mut std::task::Context<'_>,
        schedule: impl FnOnce(RetryTimerMessage) -> Result<(), RetryTimerFailure>,
    ) -> std::task::Poll<Result<(), RetryTimerFailure>> {
        match self.state.poll(cx.waker()) {
            RetryTimerResult::Ready => return std::task::Poll::Ready(Ok(())),
            RetryTimerResult::Disconnected => {
                return std::task::Poll::Ready(Err(RetryTimerFailure::Disconnected));
            }
            RetryTimerResult::Cancelled => return std::task::Poll::Ready(Ok(())),
            RetryTimerResult::Pending => {}
        }
        if self.scheduled {
            return std::task::Poll::Pending;
        }
        let item = RetryTimerMessage {
            due: std::time::Instant::now() + std::time::Duration::from_millis(10),
            state: std::sync::Arc::clone(&self.state),
            permit: None,
            accepted: false,
        };
        match schedule(item) {
            Ok(()) => {
                self.scheduled = true;
                std::task::Poll::Pending
            }
            Err(error) => std::task::Poll::Ready(Err(error)),
        }
    }
}

impl std::future::Future for RetryDelay {
    /// Result returned when the retry delay elapses or scheduling fails.
    type Output = Result<(), RetryTimerFailure>;

    /// Polls the delay and schedules its timer message on the first pending
    /// poll.
    ///
    /// # Parameters
    /// * `self`: Future state retaining the latest waker and timer status.
    /// * `cx`: Task context used to register the latest waker.
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.as_mut()
            .get_mut()
            .poll_with_schedule(cx, |item| retry_timer()?.schedule(item))
    }
}

impl Drop for RetryDelay {
    /// Marks the timer message cancelled and removes the retained waker.
    fn drop(&mut self) {
        self.state.cancel();
    }
}
