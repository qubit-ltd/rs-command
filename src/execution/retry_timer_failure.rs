// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Failure reasons returned when scheduling an asynchronous retry timer.
/// Describes why the asynchronous retry timer could not accept a delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryTimerFailure {
    /// The timer worker could not be started.
    Startup,
    /// All queued and pending timer slots are occupied.
    Full,
    /// The timer worker receiver has disconnected.
    Disconnected,
}
