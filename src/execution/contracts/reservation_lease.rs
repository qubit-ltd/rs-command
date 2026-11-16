// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;

/// Additional active-capacity ownership retained by a process/helper user.
pub(crate) struct ReservationLease {
    /// Stores the release value used by this execution component.
    release: Option<Box<dyn FnOnce() + Send + 'static>>,
}
impl ReservationLease {
    /// Creates a unique lease whose drop reports the user's final capacity
    /// release.
    ///
    /// # Parameters
    /// * `release`: Supplies the release value used by this operation.
    pub(crate) fn new(release: impl FnOnce() + Send + 'static) -> Self {
        Self {
            release: Some(Box::new(release)),
        }
    }
}
impl fmt::Debug for ReservationLease {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `formatter`: Supplies the formatter value used by this operation.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReservationLease")
            .field("active", &self.release.is_some())
            .finish()
    }
}

impl Drop for ReservationLease {
    /// Releases the retained scheduler capacity when the final helper lease
    /// drops.
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
    }
}
