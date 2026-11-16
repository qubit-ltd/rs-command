//! Single bounded worker retains unresolved cleanup owners through handoff.
use super::CommandControl;
use crate::execution::contracts::{RecoveryOwner, RecoveryRetryError};
use qubit_operation::{OperationId, SettlementReport, SettlementStatus};
use std::sync::{
    Arc,
    mpsc::{Receiver, SyncSender, sync_channel},
};

pub(crate) struct RecoveryItem {
    pub owner: Box<dyn RecoveryOwner>,
    pub control: Arc<CommandControl>,
    pub id: OperationId,
}
pub(crate) type RecoverySender = SyncSender<RecoveryItem>;

pub(crate) fn start_recovery_worker(capacity: usize) -> Result<RecoverySender, std::io::Error> {
    let (tx, rx) = sync_channel::<RecoveryItem>(capacity.max(1));
    std::thread::Builder::new()
        .name("qubit-command-recovery".to_owned())
        .spawn(move || recovery_loop(rx))?;
    Ok(tx)
}
fn recovery_loop(rx: Receiver<RecoveryItem>) {
    while let Ok(item) = rx.recv() {
        let summary = item.owner.summary();
        let mut backoff = std::time::Duration::from_millis(25);
        loop {
            match item.owner.retry_cleanup() {
                Ok(()) if item.owner.is_complete() => break,
                Err(RecoveryRetryError::OwnerAlreadyConsumed) if item.owner.is_complete() => break,
                _ => {
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(2));
                }
            }
        }
        let _ = item.control.release();
        let _ = item
            .control
            .settlement(Arc::new(recovery_settlement(item.id, &summary)));
    }
}
fn recovery_settlement(id: OperationId, summary: &str) -> SettlementReport {
    let effect = qubit_operation::EffectRecord {
        operation_id: id,
        resource_id: None,
        fact: qubit_operation::EffectFact::Unknown,
        description: qubit_operation::DiagnosticSummary::new(summary),
    };
    SettlementReport {
        operation_id: id,
        status: SettlementStatus::RecoveryRequired,
        released_count: 0,
        transferred_count: 0,
        recovery_count: 1,
        children: Arc::from([]),
        children_total: 0,
        children_truncated: false,
        cleanup_failures: Arc::from([]),
        cleanup_failures_total: 0,
        cleanup_failures_truncated: false,
        resource_reports: Arc::from([]),
        resource_reports_total: 0,
        resource_reports_truncated: false,
        transfers: Arc::from([]),
        transfers_total: 0,
        transfers_truncated: false,
        recoveries: Arc::from([]),
        recoveries_total: 1,
        recoveries_truncated: true,
        effects: Arc::from([effect]),
        effects_total: 1,
        effect_overflow: 0,
        external_settlement: None,
    }
}
