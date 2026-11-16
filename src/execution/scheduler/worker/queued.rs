// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::execution::Reservation;
use crate::execution::contracts::ScheduledJob;

/// Represents the queued state or contract used by managed command execution.
pub(super) struct Queued {
    /// Stores the job value used by this execution component.
    pub job: ScheduledJob,
    /// Stores the permit value used by this execution component.
    pub permit: Reservation,
    /// Stores the timeout requested value used by this execution component.
    pub timeout_requested: bool,
    /// Stores the timeout pending value used by this execution component.
    pub timeout_pending: bool,
}
