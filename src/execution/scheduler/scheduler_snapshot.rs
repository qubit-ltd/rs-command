// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Point-in-time scheduler counts for one command executor.
///
/// The counts are read under the scheduler lock but are observational only:
/// concurrent admissions, completions, waiter registration, and shutdown can
/// change them immediately after the snapshot is returned. `active` and
/// `queued` track reservations, `waiters` tracks registered capacity or busy
/// waits, `input_bytes` tracks reserved file-backed input, and
/// `residual_helpers` tracks cleanup work retaining scheduler capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerSnapshot {
    /// Number of active reservations.
    pub active: usize,
    /// Number of accepted reservations waiting for a worker.
    pub queued: usize,
    /// Number of registered retry and capacity waiters.
    pub waiters: usize,
    /// Estimated bytes held by active and queued reservations.
    pub input_bytes: usize,
    /// Number of cleanup helpers retaining capacity after operation outcome.
    pub residual_helpers: usize,
}
