// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "async")]
use crate::execution::CommandSubmitWaitFailure;
#[cfg(feature = "async")]
use crate::execution::command_executor::wait::WaitResult;
#[cfg(feature = "async")]
use crate::execution::contracts::CapacityFailure;
#[cfg(feature = "async")]
use crate::execution::scheduler::BoundedScheduler;

/// Future state for one scheduler capacity waiter.
#[cfg(feature = "async")]
pub(in crate::execution) struct CapacityWait {
    /// Scheduler that owns the waiter registry.
    pub(in crate::execution) scheduler: BoundedScheduler,
    /// Estimated input bytes checked by the capacity predicate.
    pub(in crate::execution) bytes: usize,
    /// Registration retained while capacity remains full.
    pub(in crate::execution) waiter: Option<u64>,
}

#[cfg(feature = "async")]
impl std::future::Future for CapacityWait {
    /// Result returned when capacity changes or a waiter limit is reached.
    type Output = Result<WaitResult, CommandSubmitWaitFailure>;

    /// Refreshes the waiter registration and reports the current capacity
    /// state.
    ///
    /// # Parameters
    /// * `self`: Future state retaining the scheduler registration.
    /// * `cx`: Task context used to register the latest waker.
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        match self
            .scheduler
            .refresh_capacity_waiter(self.bytes, self.waiter, cx.waker())
        {
            Ok(None) => {
                self.waiter = None;
                std::task::Poll::Ready(Ok(WaitResult::Retry))
            }
            Ok(Some(id)) => {
                self.waiter = Some(id);
                std::task::Poll::Pending
            }
            Err(CapacityFailure::QueueFull) => {
                self.waiter = None;
                std::task::Poll::Ready(Err(CommandSubmitWaitFailure::WaiterLimit))
            }
            Err(CapacityFailure::Closed) | Err(_) => {
                self.waiter = None;
                std::task::Poll::Ready(Ok(WaitResult::Closed))
            }
        }
    }
}

#[cfg(feature = "async")]
impl Drop for CapacityWait {
    /// Releases any waiter registration retained by this future.
    fn drop(&mut self) {
        if let Some(id) = self.waiter.take() {
            self.scheduler.cancel_waiter(id);
        }
    }
}
