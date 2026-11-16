// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Counter state guarded by the output budget mutex.

/// Aggregate observed-byte count and termination status.
pub(super) struct OutputBudgetState {
    /// Total bytes counted across both output streams.
    pub(super) observed: u64,
    /// Whether the configured limit has been reached.
    pub(super) exceeded: bool,
}
