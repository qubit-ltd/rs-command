// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Mutable counters, per-stream snapshots, and transfer flags for a run budget.

use std::vec::Vec;

/// State protected by the parent budget's mutex.
#[derive(Debug)]
pub(super) struct TotalOutputBudgetState {
    /// Bytes consumed across both streams, including the possible sentinel.
    pub(super) used: u128,
    /// Whether a byte beyond the configured limit was observed.
    pub(super) exceeded: bool,
    /// Whether further reads have been stopped.
    pub(super) halted: bool,
    /// Retained stdout prefix, moved out after its reader is joined.
    pub(super) stdout: Vec<u8>,
    /// Retained stderr prefix, moved out after its reader is joined.
    pub(super) stderr: Vec<u8>,
    /// Whether stdout exceeded its independent retention cap.
    pub(super) stdout_truncated: bool,
    /// Whether stderr exceeded its independent retention cap.
    pub(super) stderr_truncated: bool,
    /// Whether stdout EOF was observed.
    pub(super) stdout_complete: bool,
    /// Whether stderr EOF was observed.
    pub(super) stderr_complete: bool,
    /// Whether stdout bytes have been moved to a joined reader result.
    pub(super) stdout_transferred: bool,
    /// Whether stderr bytes have been moved to a joined reader result.
    pub(super) stderr_transferred: bool,
}
