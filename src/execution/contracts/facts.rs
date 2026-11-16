//! Independent outcome, release, and settlement publication port.
use crate::{CommandError, CommandOutput};
use qubit_operation::{EffectRecord, OperationOutcome, SettlementReport};
use std::sync::Arc;
/// Domain publication port. Implementations reject duplicate publication and keep the three facts independent.
pub trait FactSink: Send + Sync {
    fn publish_outcome(
        &self,
        outcome: OperationOutcome<CommandOutput, CommandError>,
    ) -> Result<(), FactPublishError>;
    fn confirm_input_release(&self) -> Result<(), FactPublishError>;
    fn publish_settlement(&self, report: Arc<SettlementReport>) -> Result<(), FactPublishError>;

    /// Transfers opaque unresolved backend ownership to the adapter's bounded recovery store.
    /// Success is a transfer receipt: the adapter retains the owner beyond this call and owns
    /// publication of input release plus a `RecoveryRequired` settlement carrying the real
    /// recovery/effect evidence. Rejection returns the owner so the backend can preserve it.
    fn retain_recovery_owner(
        &self,
        owner: Box<dyn RecoveryOwner>,
    ) -> Result<(), RecoveryPublishFailure>;
}

/// Backend-owned cleanup responsibility retained for later adapter recovery.
///
/// Adapters must retain this value until their bounded recovery worker completes cleanup and
/// publishes the corresponding settlement. `Drop` is not a cleanup operation.
pub trait RecoveryOwner: Send + Sync {
    /// Returns a redacted, bounded description suitable for recovery diagnostics.
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
    fn effect_evidence(
        &self,
        _operation_id: qubit_operation::OperationId,
    ) -> RecoveryEffectEvidence {
        RecoveryEffectEvidence::default()
    }

    /// Performs one bounded cleanup handoff attempt.
    ///
    /// Adapters must invoke this only on a bounded recovery/reaper worker. Success means the
    /// owner handed responsibility back to the upstream scope lifecycle; it does not claim that
    /// operating-system resources have already been released.
    fn retry_cleanup(&self) -> Result<(), RecoveryRetryError>;

    /// Reports whether the transferred cleanup responsibility has completed.
    ///
    /// This reports completion of the owner's handoff only. Adapters must still wait for any
    /// upstream operation/resource settlement required to prove release before removing the
    /// owner and its retained capacity lease.
    fn is_complete(&self) -> bool {
        false
    }
}

/// Bounded effect evidence carried by a recovery owner into external settlement.
#[derive(Clone, Debug, Default)]
pub struct RecoveryEffectEvidence {
    /// Retained effects with their real operation and resource identities.
    pub effects: Vec<EffectRecord>,
    /// Total observed effect records before this bounded list was retained.
    pub effects_total: usize,
    /// Records omitted because the owner-specific evidence bound was reached.
    pub effect_overflow: usize,
}

/// Bounded, redacted reason that a recovery cleanup attempt could not be handed off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryRetryError {
    OwnerAlreadyConsumed,
    CleanupHandoffUnavailable,
}

/// The adapter could not retain an unresolved owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryPublishError {
    StoreUnavailable,
    CapacityExceeded,
}

/// Rejected transfer that returns ownership to the backend for retry or reaper handoff.
pub struct RecoveryPublishFailure {
    pub reason: RecoveryPublishError,
    pub owner: Box<dyn RecoveryOwner>,
}

impl std::fmt::Debug for RecoveryPublishFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryPublishFailure")
            .field("reason", &self.reason)
            .field("owner_summary", &self.owner.summary())
            .finish()
    }
}
/// Publication rejected because an invariant would be violated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactPublishError {
    DuplicateOutcome,
    DuplicateRelease,
    DuplicateSettlement,
    IdentityMismatch,
}
