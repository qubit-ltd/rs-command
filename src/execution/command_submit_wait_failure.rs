// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::RetryTimerFailure;

/// Failure while waiting to retry a managed command submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandSubmitWaitFailure {
    /// The executor has reached its configured limit for concurrent waiters.
    WaiterLimit,
    /// The retry timer could not be scheduled.
    Timer(#[doc = "The timer startup, capacity, or connectivity failure."] RetryTimerFailure),
}

impl From<RetryTimerFailure> for CommandSubmitWaitFailure {
    /// Wraps a retry timer scheduling failure.
    ///
    /// # Parameters
    /// * `failure`: Retry timer failure to preserve in the submission error.
    fn from(failure: RetryTimerFailure) -> Self {
        Self::Timer(failure)
    }
}
