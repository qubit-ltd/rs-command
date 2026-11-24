// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Terminal outcomes reported by scheduler wait operations.

/// Terminal state observed while waiting for scheduler admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::execution) enum WaitOutcome {
    /// The caller should retry admission.
    Retry,
    /// The executor stopped accepting submissions.
    Closed,
    /// The operation scope requested cancellation.
    Stopped,
    /// The operation scope deadline elapsed.
    DeadlineExpired,
}
