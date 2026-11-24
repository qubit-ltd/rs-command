// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Single bounded worker retains unresolved cleanup owners through handoff.
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::sync_channel;

use qubit_operation::OperationId;
use qubit_operation::SettlementReport;
use qubit_operation::SettlementStatus;

use super::PendingRecovery;
use super::RecoveryItem;
use super::RecoverySender;
use crate::execution::contracts::RecoveryEffectEvidence;
use crate::execution::contracts::RecoveryRetryError;

const MAX_RECOVERY_EFFECTS: usize = 64;

/// Stores the start recovery worker value used by this execution component.
///
/// # Parameters
/// * `capacity`: Supplies the capacity value used by this operation.
pub(crate) fn start_recovery_worker(capacity: usize) -> Result<RecoverySender, std::io::Error> {
    let capacity = capacity.max(1);
    let (tx, rx) = sync_channel::<RecoveryItem>(capacity);
    let recovery = RecoverySender::new(tx, capacity);
    let outstanding = recovery.outstanding();
    std::thread::Builder::new()
        .name("qubit-command-recovery".to_owned())
        .spawn(move || recovery_loop(rx, outstanding))?;
    Ok(recovery)
}
/// Advances every accepted owner without blocking on another owner's cleanup
/// or upstream settlement.
///
/// # Parameters
/// * `rx`: Channel receiver for newly accepted owners.
/// * `outstanding`: Shared count covering queued and pending owners.
fn recovery_loop(rx: Receiver<RecoveryItem>, outstanding: Arc<AtomicUsize>) {
    let mut pending: Vec<PendingRecovery> = Vec::new();
    loop {
        while let Ok(item) = rx.try_recv() {
            let effect_evidence = item.owner.effect_evidence(item.id);
            pending.push(PendingRecovery::new(item, effect_evidence));
        }
        if pending.is_empty() {
            match rx.recv() {
                Ok(item) => {
                    let effect_evidence = item.owner.effect_evidence(item.id);
                    pending.push(PendingRecovery::new(item, effect_evidence));
                }
                Err(_) => break,
            }
        }
        let now = std::time::Instant::now();
        let mut index = 0;
        while index < pending.len() {
            let task = &mut pending[index];
            if task.next_attempt > now {
                index += 1;
                continue;
            }
            if !task.handed_off {
                match task.item.owner.retry_cleanup() {
                    Ok(()) if task.item.owner.is_complete() => task.handed_off = true,
                    Err(RecoveryRetryError::OwnerAlreadyConsumed) if task.item.owner.is_complete() => {
                        task.handed_off = true
                    }
                    _ => {
                        task.next_attempt = now + task.backoff;
                        task.backoff = (task.backoff * 2).min(std::time::Duration::from_secs(2));
                        index += 1;
                        continue;
                    }
                }
                log::debug!(
                    "recovery owner handed responsibility upstream: {}",
                    task.item.owner.summary()
                );
                let _ = task.item.control.release();
                let _ = task.item.control.settlement(Arc::new(recovery_settlement(
                    task.item.id,
                    task.effect_evidence.clone(),
                )));
            }
            if let Some(observer) = task.item.observer.try_get() {
                match observer.wait_settlement_for(std::time::Duration::ZERO) {
                    Ok(qubit_operation::SettlementWait::Ready(_)) => {
                        pending.swap_remove(index);
                        outstanding.fetch_sub(1, Ordering::AcqRel);
                        continue;
                    }
                    Ok(qubit_operation::SettlementWait::Pending(_)) => {}
                    Err(error) => log::error!("recovery worker could not observe upstream settlement: {error:?}"),
                }
            }
            task.next_attempt = now + task.backoff;
            task.backoff = (task.backoff * 2).min(std::time::Duration::from_secs(2));
            index += 1;
        }
        let timeout = pending
            .iter()
            .map(|task| task.next_attempt.saturating_duration_since(std::time::Instant::now()))
            .min()
            .unwrap_or(std::time::Duration::from_secs(2));
        match rx.recv_timeout(timeout) {
            Ok(item) => {
                let effect_evidence = item.owner.effect_evidence(item.id);
                pending.push(PendingRecovery::new(item, effect_evidence));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) if !pending.is_empty() => std::thread::sleep(timeout),
            Err(_) => {}
        }
    }
}
/// Performs the recovery settlement step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `id`: Supplies the id value used by this operation.
/// * `effect_evidence`: Supplies the effect evidence value used by this
///   operation.
fn recovery_settlement(id: OperationId, mut effect_evidence: RecoveryEffectEvidence) -> SettlementReport {
    for effect in &mut effect_evidence.effects {
        effect.operation_id = id;
    }
    let observed_total = effect_evidence.effects_total.max(
        effect_evidence
            .effects
            .len()
            .saturating_add(effect_evidence.effect_overflow),
    );
    let retained_effects: Vec<_> = effect_evidence.effects.into_iter().take(MAX_RECOVERY_EFFECTS).collect();
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
    use std::sync::Arc;
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::Duration;
    use std::time::Instant;

    use qubit_operation::DiagnosticSummary;
    use qubit_operation::EffectFact;
    use qubit_operation::EffectRecord;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::RecoveryItem;
    use super::recovery_settlement;
    use super::start_recovery_worker;
    use crate::execution::adapter::CommandControl;
    use crate::execution::adapter::OperationObserverSlot;
    use crate::execution::contracts::RecoveryEffectEvidence;
    use crate::execution::contracts::RecoveryOwner;
    use crate::execution::contracts::RecoveryRetryError;
    use crate::execution::contracts::ReservationLease;

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

    struct BlockingRecoveryOwner {
        entered: mpsc::SyncSender<()>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl RecoveryOwner for BlockingRecoveryOwner {
        fn summary(&self) -> String {
            "blocking capacity test owner".to_owned()
        }

        fn retry_cleanup(&self) -> Result<(), RecoveryRetryError> {
            self.entered.send(()).unwrap();
            let (released, changed) = &*self.gate;
            let mut released = released.lock().unwrap();
            while !*released {
                released = changed.wait(released).unwrap();
            }
            Ok(())
        }

        fn is_complete(&self) -> bool {
            true
        }
    }
    impl RecoveryOwner for CompleteOwner {
        fn summary(&self) -> String {
            "test owner".to_owned()
        }
        fn effect_evidence(&self, _operation_id: qubit_operation::OperationId) -> RecoveryEffectEvidence {
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
    fn test_recovery_lease_survives_handoff_until_manager_settlement() {
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
        while OperationControl::snapshot(&*control).settlement.is_none() && Instant::now() < deadline {
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
        let report = match operation.wait_settlement_for(Duration::from_secs(5)).unwrap() {
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
    fn test_recovery_worker_advances_independent_owner_while_prior_settlement_is_pending() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let (drop_started_tx, drop_started_rx) = mpsc::sync_channel(1);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let cleanup_gate = Arc::clone(&gate);
        let mut pending_operation = manager
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
            pending_operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        drop_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first upstream settlement should remain pending");
        let pending_id = pending_operation.id();

        let mut completed_operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        assert!(matches!(
            completed_operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        completed_operation.wait_settlement().unwrap();
        let completed_id = completed_operation.id();

        let pending_control = Arc::new(CommandControl::default());
        let completed_control = Arc::new(CommandControl::default());
        assert!(pending_control.bind(pending_id));
        assert!(completed_control.bind(completed_id));
        let pending_observer = Arc::new(OperationObserverSlot::default());
        pending_observer.set(pending_operation.observer());
        let completed_observer = Arc::new(OperationObserverSlot::default());
        completed_observer.set(completed_operation.observer());
        let recovery = start_recovery_worker(2).unwrap();

        for (id, control, observer) in [
            (pending_id, Arc::clone(&pending_control), pending_observer),
            (completed_id, Arc::clone(&completed_control), completed_observer),
        ] {
            recovery
                .send(RecoveryItem {
                    owner: Box::new(CompleteOwner {
                        _lease: Mutex::new(None),
                        evidence: RecoveryEffectEvidence::default(),
                    }),
                    control,
                    id,
                    observer,
                })
                .unwrap();
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        while OperationControl::snapshot(&*completed_control).settlement.is_none() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            OperationControl::snapshot(&*completed_control).settlement.is_some(),
            "independent owner should settle before the first upstream owner"
        );
        assert!(matches!(
            pending_operation.wait_settlement_for(Duration::ZERO).unwrap(),
            qubit_operation::SettlementWait::Pending(_)
        ));

        let (released, changed) = &*gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
        pending_operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_recovery_capacity_includes_one_active_owner_plus_channel_capacity() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let recovery = start_recovery_worker(1).unwrap();
        let make_item = || {
            let mut operation = manager
                .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
                .unwrap();
            assert!(matches!(
                operation.wait_outcome().unwrap(),
                OperationOutcome::Succeeded(())
            ));
            let id = operation.id();
            operation.wait_settlement().unwrap();
            let control = Arc::new(CommandControl::default());
            assert!(control.bind(id));
            let observer = Arc::new(OperationObserverSlot::default());
            observer.set(operation.observer());
            (
                RecoveryItem {
                    owner: Box::new(CompleteOwner {
                        _lease: Mutex::new(None),
                        evidence: RecoveryEffectEvidence::default(),
                    }),
                    control: Arc::clone(&control),
                    id,
                    observer,
                },
                control,
            )
        };

        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (mut active, active_control) = make_item();
        active.owner = Box::new(BlockingRecoveryOwner {
            entered: entered_tx,
            gate: Arc::clone(&gate),
        });
        recovery.try_send(active).unwrap();
        entered_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("worker should hold the first item as active");

        let (queued, queued_control) = make_item();
        recovery
            .try_send(queued)
            .expect("channel capacity should accept one item beside the active owner");
        let (overflow, _) = make_item();
        assert!(matches!(
            recovery.try_send(overflow),
            Err(std::sync::mpsc::TrySendError::Full(_))
        ));
        assert_eq!(recovery.outstanding().load(Ordering::Acquire), 2);

        let (released, changed) = &*gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
        let deadline = Instant::now() + Duration::from_secs(5);
        while (OperationControl::snapshot(&*active_control).settlement.is_none()
            || OperationControl::snapshot(&*queued_control).settlement.is_none())
            && Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert!(OperationControl::snapshot(&*active_control).settlement.is_some());
        assert!(OperationControl::snapshot(&*queued_control).settlement.is_some());
    }

    #[test]
    fn test_recovery_worker_bounds_owner_effects_and_preserves_overflow() {
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
            assert!(Instant::now() < deadline, "recovery report should be published");
            std::thread::yield_now();
        };

        assert_eq!(report.effects.len(), 64);
        assert_eq!(report.effects_total, 71);
        assert_eq!(report.effect_overflow, 7);
        assert_eq!(report.recovery_count, 1);
        assert!(report.effects.iter().all(|effect| effect.operation_id == id));
    }

    #[test]
    fn test_recovery_settlement_caps_overflow_to_total_minus_retained() {
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
