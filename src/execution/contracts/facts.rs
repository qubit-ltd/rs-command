//! Independent outcome, release, and settlement publication port.
use crate::{CommandError, CommandOutput};
use qubit_operation::{OperationOutcome, SettlementReport};
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

    /// Performs one bounded cleanup handoff attempt.
    ///
    /// Adapters must invoke this only on a bounded recovery/reaper worker. Success means the
    /// owner handed responsibility back to the upstream scope lifecycle; it does not claim that
    /// operating-system resources have already been released.
    fn retry_cleanup(&self) -> Result<(), RecoveryRetryError>;
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
