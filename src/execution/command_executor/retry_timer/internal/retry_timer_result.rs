// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Terminal result states for one retry timer request.

/// The result visible to a retry future for its timer request.
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
