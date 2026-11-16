//! Single bounded worker retains unresolved cleanup owners through handoff.
use super::CommandControl;
use crate::execution::contracts::{RecoveryEffectEvidence, RecoveryOwner, RecoveryRetryError};
use qubit_operation::{OperationId, SettlementReport, SettlementStatus};
use std::sync::{
    Arc,
    mpsc::{Receiver, SyncSender, sync_channel},
};

const MAX_RECOVERY_EFFECTS: usize = 64;

pub(crate) struct RecoveryItem {
    pub owner: Box<dyn RecoveryOwner>,
    pub control: Arc<CommandControl>,
    pub id: OperationId,
    pub observer: Arc<OperationObserverSlot>,
}
pub(crate) type RecoverySender = SyncSender<RecoveryItem>;

#[derive(Default)]
pub(crate) struct OperationObserverSlot {
    observer: std::sync::Mutex<Option<qubit_operation::OperationObserver>>,
    available: std::sync::Condvar,
}
impl OperationObserverSlot {
    pub(crate) fn set(&self, observer: qubit_operation::OperationObserver) {
        *self
            .observer
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(observer);
        self.available.notify_all();
    }

    fn wait(&self) -> qubit_operation::OperationObserver {
        let mut observer = self
            .observer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while observer.is_none() {
            observer = self
                .available
                .wait(observer)
                .unwrap_or_else(|error| error.into_inner());
        }
        observer.as_ref().expect("observer is set").clone()
    }
}

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
        let effect_evidence = item.owner.effect_evidence(item.id);
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
        log::debug!("recovery owner handed responsibility upstream: {summary}");
        let _ = item.control.release();
        let _ = item
            .control
            .settlement(Arc::new(recovery_settlement(item.id, effect_evidence)));
        let observer = item.observer.wait();
        loop {
            match observer.wait_settlement() {
                Ok(_) => break,
                Err(error) => {
                    log::error!("recovery worker could not observe upstream settlement: {error:?}");
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(2));
                }
            }
        }
    }
}
fn recovery_settlement(
    id: OperationId,
    mut effect_evidence: RecoveryEffectEvidence,
) -> SettlementReport {
    for effect in &mut effect_evidence.effects {
        effect.operation_id = id;
    }
    let observed_total = effect_evidence.effects_total.max(
        effect_evidence
            .effects
            .len()
            .saturating_add(effect_evidence.effect_overflow),
    );
    let retained_effects: Vec<_> = effect_evidence
        .effects
        .into_iter()
        .take(MAX_RECOVERY_EFFECTS)
        .collect();
    let effects_total = observed_total.max(retained_effects.len());
    // The normalized total covers all producer-reported and adapter-omitted
    // records, so overflow is exactly the portion not retained here.
    let effect_overflow = effects_total.saturating_sub(retained_effects.len());
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
        recoveries_total: 0,
        recoveries_truncated: false,
        effects: Arc::from(retained_effects),
        effects_total,
        effect_overflow,
        external_settlement: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationObserverSlot, RecoveryItem, recovery_settlement, start_recovery_worker};
    use crate::execution::adapter::CommandControl;
    use crate::execution::contracts::{
        RecoveryEffectEvidence, RecoveryOwner, RecoveryRetryError, ReservationLease,
    };
    use qubit_operation::{
        DiagnosticSummary, EffectFact, EffectRecord, OperationControl, OperationManager,
        OperationOptions, OperationOutcome,
    };
    use std::sync::{Arc, Condvar, Mutex, mpsc};
    use std::time::{Duration, Instant};

    struct BlockingDrop {
        started: mpsc::SyncSender<()>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }
    impl Drop for BlockingDrop {
        fn drop(&mut self) {
            let _ = self.started.send(());
            let (released, changed) = &*self.gate;
            let mut released = released.lock().unwrap();
            while !*released {
                released = changed.wait(released).unwrap();
            }
        }
    }

    struct CompleteOwner {
        _lease: Mutex<Option<ReservationLease>>,
        evidence: RecoveryEffectEvidence,
    }
    impl RecoveryOwner for CompleteOwner {
        fn summary(&self) -> String {
            "test owner".to_owned()
        }
        fn effect_evidence(
            &self,
            _operation_id: qubit_operation::OperationId,
        ) -> RecoveryEffectEvidence {
            self.evidence.clone()
        }
        fn retry_cleanup(&self) -> Result<(), RecoveryRetryError> {
            Err(RecoveryRetryError::OwnerAlreadyConsumed)
        }
        fn is_complete(&self) -> bool {
            true
        }
    }

    #[test]
    fn recovery_lease_survives_handoff_until_manager_settlement() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let (drop_started_tx, drop_started_rx) = mpsc::sync_channel(1);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let cleanup_gate = Arc::clone(&gate);
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let resource = scope
                    .manage(BlockingDrop {
                        started: drop_started_tx,
                        gate: cleanup_gate,
                    })
                    .unwrap();
                drop(resource);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        drop_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("scope cleanup should reach the gate");

        let id = operation.id();
        let control = Arc::new(CommandControl::default());
        assert!(control.bind(id));
        let (lease_dropped_tx, lease_dropped_rx) = mpsc::sync_channel(1);
        let lease = ReservationLease::new(move || {
            let _ = lease_dropped_tx.send(());
        });
        let observer = Arc::new(OperationObserverSlot::default());
        let recovery = start_recovery_worker(1).unwrap();
        recovery
            .send(RecoveryItem {
                owner: Box::new(CompleteOwner {
                    _lease: Mutex::new(Some(lease)),
                    evidence: RecoveryEffectEvidence {
                        effects: vec![EffectRecord {
                            operation_id: id,
                            resource_id: None,
                            fact: EffectFact::Unknown,
                            description: DiagnosticSummary::new("owner-held effect evidence"),
                        }],
                        effects_total: 3,
                        effect_overflow: 2,
                    },
                }),
                control: Arc::clone(&control),
                id,
                observer: Arc::clone(&observer),
            })
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while OperationControl::snapshot(&*control).settlement.is_none()
            && Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert!(OperationControl::snapshot(&*control).input_released);
        let external_report = OperationControl::snapshot(&*control)
            .settlement
            .expect("recovery worker publishes external settlement");
        assert_eq!(external_report.recovery_count, 1);
        assert_eq!(external_report.effects.len(), 1);
        assert_eq!(external_report.effects_total, 3);
        assert_eq!(external_report.effect_overflow, 2);
        assert_eq!(external_report.effects[0].operation_id, id);
        assert_eq!(external_report.effects[0].resource_id, None);
        assert_eq!(external_report.effects[0].fact, EffectFact::Unknown);
        assert_eq!(
            external_report.effects[0].description.as_str(),
            "owner-held effect evidence"
        );
        assert!(lease_dropped_rx.try_recv().is_err());
        assert!(matches!(
            operation.wait_settlement_for(Duration::ZERO).unwrap(),
            qubit_operation::SettlementWait::Pending(_)
        ));
        observer.set(operation.observer());
        assert!(lease_dropped_rx.try_recv().is_err());

        let (released, changed) = &*gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
        let report = match operation
            .wait_settlement_for(Duration::from_secs(5))
            .unwrap()
        {
            qubit_operation::SettlementWait::Ready(report) => report,
            qubit_operation::SettlementWait::Pending(_) => {
                panic!("scope must settle after gate release")
            }
        };
        assert_eq!(report.resource_reports_total, 1);
        lease_dropped_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("recovery owner lease drops only after manager settlement");
    }

    #[test]
    fn recovery_worker_bounds_owner_effects_and_preserves_overflow() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let _ = operation.wait_outcome().unwrap();
        let id = operation.id();
        operation.wait_settlement().unwrap();
        let mut foreign_operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let _ = foreign_operation.wait_outcome().unwrap();
        let foreign_id = foreign_operation.id();
        foreign_operation.wait_settlement().unwrap();

        let effects = (0..70)
            .map(|_| EffectRecord {
                operation_id: foreign_id,
                resource_id: None,
                fact: EffectFact::Unknown,
                description: DiagnosticSummary::new("boundedness evidence"),
            })
            .collect();
        let control = Arc::new(CommandControl::default());
        assert!(control.bind(id));
        let observer = Arc::new(OperationObserverSlot::default());
        observer.set(operation.observer());
        let recovery = start_recovery_worker(1).unwrap();
        recovery
            .send(RecoveryItem {
                owner: Box::new(CompleteOwner {
                    _lease: Mutex::new(None),
                    evidence: RecoveryEffectEvidence {
                        effects,
                        effects_total: 2,
                        effect_overflow: 1,
                    },
                }),
                control: Arc::clone(&control),
                id,
                observer,
            })
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let report = loop {
            if let Some(report) = OperationControl::snapshot(&*control).settlement {
                break report;
            }
            assert!(
                Instant::now() < deadline,
                "recovery report should be published"
            );
            std::thread::yield_now();
        };

        assert_eq!(report.effects.len(), 64);
        assert_eq!(report.effects_total, 71);
        assert_eq!(report.effect_overflow, 7);
        assert_eq!(report.recovery_count, 1);
        assert!(
            report
                .effects
                .iter()
                .all(|effect| effect.operation_id == id)
        );
    }

    #[test]
    fn recovery_settlement_caps_overflow_to_total_minus_retained() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let _ = operation.wait_outcome().unwrap();
        let id = operation.id();
        operation.wait_settlement().unwrap();

        let report = recovery_settlement(
            id,
            RecoveryEffectEvidence {
                effects: vec![EffectRecord {
                    operation_id: id,
                    resource_id: None,
                    fact: EffectFact::Unknown,
                    description: DiagnosticSummary::new("max overflow evidence"),
                }],
                effects_total: 0,
                effect_overflow: usize::MAX,
            },
        );

        assert_eq!(report.effects_total, usize::MAX);
        assert_eq!(report.effects.len(), 1);
        assert_eq!(report.effect_overflow, usize::MAX - 1);
        assert!(report.effect_overflow <= report.effects_total - report.effects.len());
    }
}
