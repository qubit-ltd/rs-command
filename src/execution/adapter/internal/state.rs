// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::task::Waker;

use qubit_operation::ChildOutcomeSummary;
use qubit_operation::ChildRelease;
use qubit_operation::ExecutionPhase;
use qubit_operation::OperationId;
use qubit_operation::OperationOutcome;
use qubit_operation::SettlementReport;
use qubit_operation::StopReport;

use crate::CommandError;
use crate::CommandOutput;

/// Represents the state state or contract used by managed command execution.
pub(in crate::execution::adapter) struct State {
    /// Stores the id value used by this execution component.
    pub(in crate::execution::adapter) id: Option<OperationId>,
    /// Stores the phase value used by this execution component.
    pub(in crate::execution::adapter) phase: ExecutionPhase,
    /// Stores the stop value used by this execution component.
    pub(in crate::execution::adapter) stop: Option<StopReport>,
    /// Stores the outcome value used by this execution component.
    pub(in crate::execution::adapter) outcome: Option<OperationOutcome<CommandOutput, CommandError>>,
    /// Stores the summary value used by this execution component.
    pub(in crate::execution::adapter) summary: Option<ChildOutcomeSummary>,
    /// Stores the release value used by this execution component.
    pub(in crate::execution::adapter) release: Option<ChildRelease>,
    /// Stores the settlement value used by this execution component.
    pub(in crate::execution::adapter) settlement: Option<Arc<SettlementReport>>,
    /// Stores the outcome wakers value used by this execution component.
    pub(in crate::execution::adapter) outcome_wakers: Vec<Waker>,
    /// Stores the release wakers value used by this execution component.
    pub(in crate::execution::adapter) release_wakers: Vec<Waker>,
    /// Stores the settlement wakers value used by this execution component.
    pub(in crate::execution::adapter) settlement_wakers: Vec<Waker>,
}
