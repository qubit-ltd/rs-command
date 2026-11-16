// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Represents the scheduler snapshot state or contract used by managed command
/// execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerSnapshot {
    /// Stores the active value used by this execution component.
    pub active: usize,
    /// Stores the queued value used by this execution component.
    pub queued: usize,
    /// Stores the waiters value used by this execution component.
    pub waiters: usize,
    /// Stores the input bytes value used by this execution component.
    pub input_bytes: usize,
    /// Stores the residual helpers value used by this execution component.
    pub residual_helpers: usize,
}
