// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;

use super::CapacityFailure;
use super::Reservation;
use super::ScheduledJob;

/// Rejection returns both job and permit so the caller can recover ownership.
pub struct EnqueueFailure {
    /// Stores the reason value used by this execution component.
    pub reason: CapacityFailure,
    /// Stores the job value used by this execution component.
    pub job: ScheduledJob,
    /// Stores the permit value used by this execution component.
    pub permit: Reservation,
}
impl fmt::Debug for EnqueueFailure {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `f`: Supplies the f value used by this operation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnqueueFailure")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}
