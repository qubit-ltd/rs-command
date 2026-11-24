// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use qubit_operation::OperationContext;

use super::total_output_budget::TotalOutputBudget;
use crate::CommandCancellation;
use crate::execution::backend::ReaperObserver;
use crate::execution::backend::StartGate;
use crate::execution::contracts::ReservationLease;
use crate::session::contracts::SessionBridge;

/// Resources retained by a running command alongside its process and I/O.
pub(in crate::command_runner) struct RunningCommandResources {
    /// Shared cancellation handle checked while waiting.
    pub(in crate::command_runner) cancellation_token: Option<CommandCancellation>,
    /// Operation-local stop signal independent of caller-owned cancellation.
    pub(in crate::command_runner) operation_cancellation_token: Option<CommandCancellation>,
    /// Managed gate updated when runtime cancellation is observed.
    pub(in crate::command_runner) start_gate: Option<Arc<StartGate>>,
    /// Shared byte budget charged by stdout and stderr.
    pub(in crate::command_runner) total_budget: Option<Arc<TotalOutputBudget>>,
    /// Managed capacity retained through cleanup.
    pub(in crate::command_runner) capacity_lease: Option<ReservationLease>,
    /// Observer notified when ownership is handed to a reaper.
    pub(in crate::command_runner) reaper_observer: Option<ReaperObserver>,
    /// Session bridge stopped when command I/O is reclaimed.
    pub(in crate::command_runner) session_bridge: Option<Arc<dyn SessionBridge>>,
    /// Managed operation context propagated to the child lifecycle.
    pub(in crate::command_runner) operation_context: Option<OperationContext>,
}
