// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_operation::OperationOutcome;
use qubit_operation::SettlementReport;

use super::FactPublishError;
use super::RecoveryOwner;
use super::RecoveryPublishFailure;
use crate::CommandError;
use crate::CommandOutput;

/// Domain publication port. Implementations reject duplicate publication and
/// keep the three facts independent.
pub trait FactSink: Send + Sync {
    /// Performs the publish outcome step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `outcome`: Supplies the outcome value used by this operation.
    fn publish_outcome(
        &self,
        outcome: OperationOutcome<CommandOutput, CommandError>,
    ) -> Result<(), FactPublishError>;
    /// Performs the confirm input release step in the managed command execution
    /// lifecycle.
    fn confirm_input_release(&self) -> Result<(), FactPublishError>;
    /// Stores the publish settlement value used by this execution component.
    ///
    /// # Parameters
    /// * `report`: Supplies the report value used by this operation.
    fn publish_settlement(&self, report: Arc<SettlementReport>) -> Result<(), FactPublishError>;

    /// Transfers opaque unresolved backend ownership to the adapter's bounded
    /// recovery store. Success is a transfer receipt: the adapter retains
    /// the owner beyond this call and owns publication of input release
    /// plus a `RecoveryRequired` settlement carrying the real
    /// recovery/effect evidence. Rejection returns the owner so the backend can
    /// preserve it.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    fn retain_recovery_owner(
        &self,
        owner: Box<dyn RecoveryOwner>,
    ) -> Result<(), RecoveryPublishFailure>;
}
