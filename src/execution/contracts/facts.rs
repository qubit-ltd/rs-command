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
}
/// Publication rejected because an invariant would be violated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactPublishError {
    DuplicateOutcome,
    DuplicateRelease,
    DuplicateSettlement,
    IdentityMismatch,
}
