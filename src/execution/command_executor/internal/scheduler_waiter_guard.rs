// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

#[cfg(feature = "async")]
use crate::execution::scheduler::BoundedScheduler;

/// Owns one scheduler waiter registration for an async retry phase.
#[cfg(feature = "async")]
pub(in crate::execution) struct SchedulerWaiterGuard {
    /// Scheduler that owns the registration.
    pub(in crate::execution) scheduler: BoundedScheduler,
    /// Identifier released when this guard is dropped.
    pub(in crate::execution) id: u64,
}

#[cfg(feature = "async")]
impl Drop for SchedulerWaiterGuard {
    fn drop(&mut self) {
        self.scheduler.cancel_waiter(self.id);
    }
}
