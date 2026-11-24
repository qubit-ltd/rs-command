// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Terminal outcomes shared by executor wait paths.

/// Result observed at the wait termination point.
pub(in crate::execution) enum WaitResult {
    /// Admission may be retried.
    Retry,
    /// The executor has closed.
    Closed,
    /// The operation was stopped.
    Stopped,
    /// The operation deadline elapsed.
    DeadlineExpired,
    /// Waiting failed before admission could be retried.
    Failed(
        /// The waiter quota or retry timer failure that ended the wait.
        crate::execution::CommandSubmitWaitFailure,
    ),
    /// The submission reason does not support waiting.
    NotRetryable,
}
