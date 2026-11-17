// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::RecoveryEffectEvidence;
use super::RecoveryRetryError;

/// Backend-owned cleanup responsibility retained for later adapter recovery.
///
/// Adapters must retain this value until their bounded recovery worker
/// completes cleanup and publishes the corresponding settlement. `Drop` is not
/// a cleanup operation.
pub trait RecoveryOwner: Send + Sync {
    /// Returns a redacted, bounded description suitable for recovery
    /// diagnostics.
    fn summary(&self) -> String;

    /// Returns bounded, factual effect evidence retained by this owner.
    ///
    /// This is domain evidence for the external settlement report. Upstream
    /// resource-scope cleanup records remain in the manager's final report and
    /// must not be copied here. Owners without a known resource effect return
    /// the empty default; they must not invent resource identities. Producers
    /// should retain at most 64 effects and report omitted records through
    /// `effects_total` and `effect_overflow`, and every record should use the
    /// supplied `operation_id`. The adapter enforces the same bound and
    /// canonicalizes record identities at the settlement boundary.
    ///
    /// # Parameters
    /// * `_operation_id`: Supplies the  operation id value used by this
    ///   operation.
    fn effect_evidence(
        &self,
        _operation_id: qubit_operation::OperationId,
    ) -> RecoveryEffectEvidence {
        RecoveryEffectEvidence::default()
    }

    /// Performs one bounded cleanup handoff attempt.
    ///
    /// Adapters must invoke this only on a bounded recovery/reaper worker.
    /// Success means the owner handed responsibility back to the upstream
    /// scope lifecycle; it does not claim that operating-system resources
    /// have already been released.
    fn retry_cleanup(&self) -> Result<(), RecoveryRetryError>;

    /// Reports whether the transferred cleanup responsibility has completed.
    ///
    /// This reports completion of the owner's handoff only. Adapters must still
    /// wait for any upstream operation/resource settlement required to
    /// prove release before removing the owner and its retained capacity
    /// lease.
    fn is_complete(&self) -> bool {
        false
    }
}
