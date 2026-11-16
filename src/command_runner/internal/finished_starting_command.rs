// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use super::command_io::CommandIo;
use super::managed_child_process::ManagedChildProcess;
use crate::execution::backend::ReaperObserver;
use crate::execution::contracts::ReservationLease;
use crate::session::contracts::SessionBridge;

/// Fully initialized child state transferred from startup to execution.
pub(in crate::command_runner) struct FinishedStartingCommand {
    /// Spawned child transferred into running state.
    pub(in crate::command_runner) child_process: ManagedChildProcess,
    /// Initialized standard-I/O helpers owned by the command.
    pub(in crate::command_runner) io: CommandIo,
    /// Managed capacity retained until cleanup completes.
    pub(in crate::command_runner) capacity_lease: Option<ReservationLease>,
    /// Observer for any later process ownership handoff.
    pub(in crate::command_runner) reaper_observer: Option<ReaperObserver>,
    /// Session bridge used by the running I/O helpers.
    pub(in crate::command_runner) session_bridge: Option<Arc<dyn SessionBridge>>,
}
