// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::ReaperObserver;
use super::ReaperOwner;
use crate::execution::backend::Reaper;
use crate::execution::contracts::RecoveryEffectEvidence;
use crate::execution::contracts::RecoveryOwner;

/// Represents the deferred reaper owner state or contract used by managed
/// command execution.
pub(crate) struct DeferredReaperOwner {
    /// Stores the owner value used by this execution component.
    owner: std::sync::Mutex<Option<ReaperOwner>>,
    /// Stores the observer value used by this execution component.
    observer: ReaperObserver,
}

impl DeferredReaperOwner {
    /// Stores the new value used by this execution component.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    /// * `observer`: Supplies the observer value used by this operation.
    pub(crate) fn new(owner: ReaperOwner, observer: ReaperObserver) -> Self {
        Self {
            owner: std::sync::Mutex::new(Some(owner)),
            observer,
        }
    }
}

impl RecoveryOwner for DeferredReaperOwner {
    /// Performs the summary step in the managed command execution lifecycle.
    fn summary(&self) -> String {
        "managed child and I/O helpers awaiting bounded reaper capacity".to_owned()
    }

    /// Performs the effect evidence step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `operation_id`: Supplies the operation id value used by this
    ///   operation.
    fn effect_evidence(&self, operation_id: qubit_operation::OperationId) -> RecoveryEffectEvidence {
        if self.observer.is_complete() || !self.observer.is_pending() {
            return RecoveryEffectEvidence::default();
        }
        let queued = self.owner.lock().unwrap_or_else(|error| error.into_inner()).is_none();
        let description = if queued {
            "managed process/helper cleanup is pending in reaper"
        } else {
            "managed process/helper cleanup awaits reaper handoff"
        };
        RecoveryEffectEvidence {
            effects: vec![qubit_operation::EffectRecord {
                operation_id,
                resource_id: None,
                fact: qubit_operation::EffectFact::Unknown,
                description: qubit_operation::DiagnosticSummary::new(description),
            }],
            effects_total: 1,
            effect_overflow: 0,
        }
    }

    /// Stores the retry cleanup value used by this execution component.
    fn retry_cleanup(&self) -> Result<(), crate::execution::contracts::RecoveryRetryError> {
        let mut slot = self.owner.lock().unwrap_or_else(|error| error.into_inner());
        let Some(owner) = slot.take() else {
            return if self.observer.is_complete() {
                Ok(())
            } else {
                Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
            };
        };
        let result = match Reaper::global() {
            Ok(reaper) => reaper.handoff(owner),
            Err(_) => Err(owner),
        };
        match result {
            Ok(()) => Ok(()),
            Err(owner) => {
                *slot = Some(owner);
                Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
            }
        }
    }

    /// Performs the is complete step in the managed command execution
    /// lifecycle.
    fn is_complete(&self) -> bool {
        self.observer.is_complete()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;

    use super::DeferredReaperOwner;
    use super::ReaperObserver;
    use super::ReaperOwner;
    use crate::execution::contracts::RecoveryOwner;

    #[test]
    fn test_unstarted_cleanup_does_not_report_unknown_effects() {
        let (observer, _completed) = ReaperObserver::pair();
        let owner = DeferredReaperOwner::new(ReaperOwner::new(|| {}), observer.clone());
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();

        assert!(!owner.is_complete());
        assert_eq!(
            owner.summary(),
            "managed child and I/O helpers awaiting bounded reaper capacity"
        );
        assert!(owner.effect_evidence(operation.id()).effects.is_empty());

        observer.mark_pending();
        let evidence = owner.effect_evidence(operation.id());
        assert_eq!(evidence.effects_total, 1);
        assert_eq!(evidence.effects[0].fact, qubit_operation::EffectFact::Unknown);

        observer.complete();
        assert!(owner.is_complete());
        assert!(owner.effect_evidence(operation.id()).effects.is_empty());
        assert!(operation.wait_outcome().is_ok());
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_cleanup_retry_moves_owner_once_and_reports_handoff_state() {
        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let (cleanup_tx, cleanup_rx) = mpsc::channel();
        let cleanup_observer = observer.clone();
        let owner = DeferredReaperOwner::new(
            ReaperOwner::new(move || {
                cleanup_rx.recv().unwrap();
                cleanup_observer.complete();
            }),
            observer.clone(),
        );
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let id = operation.id();

        let awaiting_handoff = owner.effect_evidence(id);
        assert_eq!(awaiting_handoff.effects_total, 1);
        assert_eq!(
            awaiting_handoff.effects[0].description.as_str(),
            "managed process/helper cleanup awaits reaper handoff"
        );

        owner.retry_cleanup().unwrap();
        let awaiting_reaper = owner.effect_evidence(id);
        assert_eq!(awaiting_reaper.effects_total, 1);
        assert_eq!(
            awaiting_reaper.effects[0].description.as_str(),
            "managed process/helper cleanup is pending in reaper"
        );
        assert!(matches!(
            owner.retry_cleanup(),
            Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
        ));

        assert!(operation.wait_outcome().is_ok());
        operation.wait_settlement().unwrap();
        cleanup_tx.send(()).unwrap();
        completed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(owner.is_complete());
        assert!(owner.effect_evidence(id).effects.is_empty());
        owner.retry_cleanup().unwrap();
    }
}
