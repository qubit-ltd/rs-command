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

/// Waits for one scheduled retry delay and tracks its latest task waker.
pub struct RetryDelay {
    /// Whether the timer worker has completed this delay.
    pub(super) ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Whether this retry future has been dropped.
    pub(super) cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The waker registered by the most recent poll.
    pub(super) waker: std::sync::Arc<std::sync::Mutex<Option<std::task::Waker>>>,
    /// Whether a timer message has been queued.
    pub(super) scheduled: bool,
}

impl RetryDelay {
    /// Creates a new retry delay future.
    pub fn new() -> Self {
        Self {
            ready: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            waker: std::sync::Arc::new(std::sync::Mutex::new(None)),
            scheduled: false,
        }
    }

    /// Polls the delay while delegating message scheduling.
    pub(super) fn poll_with_schedule(
        &mut self,
        cx: &mut std::task::Context<'_>,
        schedule: impl FnOnce(RetryTimerMessage) -> Result<(), RetryTimerFailure>,
    ) -> std::task::Poll<Result<(), RetryTimerFailure>> {
        if self.ready.load(std::sync::atomic::Ordering::Acquire) {
            return std::task::Poll::Ready(Ok(()));
        }
        {
            let mut waker = self.waker.lock().unwrap_or_else(|e| e.into_inner());
            if !waker
                .as_ref()
                .is_some_and(|registered| registered.will_wake(cx.waker()))
            {
                *waker = Some(cx.waker().clone());
            }
        }
        if self.ready.load(std::sync::atomic::Ordering::Acquire) {
            return std::task::Poll::Ready(Ok(()));
        }
        if self.scheduled {
            return std::task::Poll::Pending;
        }
        let item = RetryTimerMessage {
            due: std::time::Instant::now() + std::time::Duration::from_millis(10),
            waker: std::sync::Arc::clone(&self.waker),
            ready: std::sync::Arc::clone(&self.ready),
            cancelled: std::sync::Arc::clone(&self.cancelled),
            permit: None,
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
    type Output = Result<(), RetryTimerFailure>;

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
    fn drop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        self.waker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }
}
