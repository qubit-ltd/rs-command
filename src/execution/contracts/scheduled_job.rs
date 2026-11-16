// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::Reservation;
use super::StopPort;

/// Domain task scheduled without borrowing an operation scope.
pub struct ScheduledJob {
    /// Runs on a domain worker and receives the unique capacity permit.
    pub run: Box<dyn FnOnce(Reservation) + Send + 'static>,
    /// Stores the stop value used by this execution component.
    pub stop: Arc<dyn StopPort>,
}
impl fmt::Debug for ScheduledJob {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `f`: Supplies the f value used by this operation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduledJob").finish_non_exhaustive()
    }
}
