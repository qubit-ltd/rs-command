// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::sync::Arc;

use super::ReservationLease;

/// Exclusive scheduler permit. Its one-shot release callback runs on drop,
/// including when the permit was never activated; moving it into
/// `ActivatedRequest` keeps it held.
pub struct Reservation {
    /// Stores the scheduler id value used by this execution component.
    pub(crate) scheduler_id: u64,
    /// Stores the token value used by this execution component.
    pub(crate) token: u64,
    /// Stores the release value used by this execution component.
    release: Option<Box<dyn FnOnce(u64) + Send + 'static>>,
    /// Stores the retain value used by this execution component.
    retain: Arc<dyn Fn(u64) -> ReservationLease + Send + Sync + 'static>,
}
impl Reservation {
    /// Creates a uniquely owned permit with its scheduler's nonblocking release
    /// hook.
    ///
    /// The release callback must be nonblocking and must not panic because it
    /// runs from `Drop`.
    ///
    /// # Parameters
    /// * `scheduler_id`: Supplies the scheduler id value used by this
    ///   operation.
    /// * `token`: Supplies the token value used by this operation.
    /// * `release`: Supplies the release value used by this operation.
    /// * `retain`: Supplies the retain value used by this operation.
    pub(crate) fn new(
        scheduler_id: u64,
        token: u64,
        release: impl FnOnce(u64) + Send + 'static,
        retain: impl Fn(u64) -> ReservationLease + Send + Sync + 'static,
    ) -> Self {
        Self {
            scheduler_id,
            token,
            release: Some(Box::new(release)),
            retain: Arc::new(retain),
        }
    }

    /// Reports whether this permit was created by the given scheduler instance.
    ///
    /// # Parameters
    pub(crate) fn belongs_to(&self, scheduler_id: u64) -> bool {
        self.scheduler_id == scheduler_id
    }

    /// Retains active capacity for one process/helper user beyond the permit's
    /// lifetime.
    pub(crate) fn retain(&self) -> ReservationLease {
        (self.retain)(self.token)
    }
}
impl fmt::Debug for Reservation {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `formatter`: Supplies the formatter value used by this operation.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Reservation")
            .field("scheduler_id", &self.scheduler_id)
            .field("token", &self.token)
            .field("active", &self.release.is_some())
            .finish()
    }
}

impl Drop for Reservation {
    /// Performs the drop step in the managed command execution lifecycle.
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release(self.token);
        }
    }
}
