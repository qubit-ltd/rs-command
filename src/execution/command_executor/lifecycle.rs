// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use super::CommandExecutor;
use super::scheduler::SchedulerSnapshot;
use crate::execution::contracts::Scheduler;

impl CommandExecutor {
    /// Returns a point-in-time count of local active, queued, and waiting work.
    ///
    /// The snapshot is observational and can become stale as soon as it is
    /// returned. It does not report operation settlement or recovery state.
    pub fn scheduler_snapshot(&self) -> SchedulerSnapshot {
        self.scheduler.snapshot()
    }

    /// Closes local admission and requests shutdown of the executor's operation
    /// group.
    ///
    /// `mode` controls how the operation manager handles accepted work. The
    /// returned handle can be used to observe group shutdown. Closing admission
    /// wakes capacity and busy waiters; it does not synchronously join worker
    /// threads or prove that accepted operations have settled.
    pub fn request_shutdown(
        &self,
        mode: qubit_operation::ShutdownMode,
    ) -> qubit_operation::ShutdownHandle {
        self.scheduler.close_admission();
        self.group.request_shutdown(mode)
    }
}
