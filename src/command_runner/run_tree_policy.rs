// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use qubit_operation::OperationContext;

use crate::ProcessTreePolicy;
use crate::execution::backend::ReaperObserver;
use crate::execution::backend::StartGate;
use crate::execution::contracts::PreparedFiles;
use crate::execution::contracts::ReservationLease;
use crate::session::contracts::SessionBridge;

/// Carries managed execution resources into a command run.
///
/// The optional values connect process-tree management, prepared files,
/// capacity accounting, reaping, sessions, and operation context without
/// changing the ordinary runner configuration.
#[derive(Default)]
pub(crate) struct RunTreePolicy<'a> {
    /// Process-tree policy selected by the managed executor.
    pub(crate) managed_policy: Option<ProcessTreePolicy>,
    /// Gate that coordinates startup with the managed operation.
    pub(crate) start_gate: Option<&'a StartGate>,
    /// Open file resources prepared for this command.
    pub(crate) prepared_files: Option<PreparedFiles>,
    /// Scope used to record side effects from the owned-file commit.
    pub(crate) file_scope: Option<&'a qubit_operation::OperationScope>,
    /// Managed capacity reservation retained through process cleanup.
    pub(crate) capacity_lease: Option<ReservationLease>,
    /// Observer notified when child ownership is handed to a reaper.
    pub(crate) reaper_observer: Option<ReaperObserver>,
    /// Optional streaming session connected to command output and input.
    pub(crate) session_bridge: Option<Arc<dyn SessionBridge>>,
    /// Context used to propagate operation lifecycle information.
    pub(crate) operation_context: Option<OperationContext>,
}
