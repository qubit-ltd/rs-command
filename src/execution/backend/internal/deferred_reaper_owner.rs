// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::ReaperObserver;
use super::ReaperOwner;
use super::inline_reaper_fallback_allowed;
use super::with_inline_reaper_fallback;
use crate::execution::backend::Reaper;
use crate::execution::contracts::RecoveryEffectEvidence;
use crate::execution::contracts::RecoveryOwner;

/// Runs a recovery attempt with inline fallback on an existing bounded worker.
/// The thread-local mode resets when the attempt returns or unwinds.
///
/// # Parameters
/// * `owner`: Recovery owner whose cleanup may use the worker's inline
///   fallback.
pub(crate) fn retry_cleanup_on_worker(
    owner: &dyn RecoveryOwner,
) -> Result<(), crate::execution::contracts::RecoveryRetryError> {
    with_inline_reaper_fallback(|| owner.retry_cleanup())
}

/// Represents the deferred reaper owner state or contract used by managed
/// command execution.
pub(crate) struct DeferredReaperOwner {
    /// Retains the cleanup owner until a reaper accepts it or local cleanup
    /// ends.
    owner: std::sync::Mutex<Option<ReaperOwner>>,
    /// Tracks the child and helper cleanup associated with this owner.
    observer: ReaperObserver,
    /// Submits the owner to the bounded reaper service.
    handoff: std::sync::Arc<dyn Fn(ReaperOwner) -> Result<(), crate::execution::backend::HandoffFailure> + Send + Sync>,
}

impl DeferredReaperOwner {
    /// Creates an owner that submits cleanup to the process-wide reaper.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    /// * `observer`: Supplies the observer value used by this operation.
    pub(crate) fn new(owner: ReaperOwner, observer: ReaperObserver) -> Self {
        Self::with_handoff(
            owner,
            observer,
            std::sync::Arc::new(|owner| Reaper::global().handoff(owner)),
        )
    }

    /// Creates an owner with an injected handoff operation for focused tests.
    ///
    /// # Parameters
    /// * `owner`: Holds the cleanup action whose ownership must be transferred.
    /// * `observer`: Reports whether the associated process and helpers are
    ///   fully cleaned up.
    /// * `handoff`: Attempts to submit the owner and returns it when submission
    ///   fails.
    fn with_handoff(
        owner: ReaperOwner,
        observer: ReaperObserver,
        handoff: std::sync::Arc<
            dyn Fn(ReaperOwner) -> Result<(), crate::execution::backend::HandoffFailure> + Send + Sync,
        >,
    ) -> Self {
        Self {
            owner: std::sync::Mutex::new(Some(owner)),
            observer,
            handoff,
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
        match (self.handoff)(owner) {
            Ok(()) => Ok(()),
            Err(failure) if failure.kind == crate::execution::backend::HandoffFailureKind::Full => {
                *slot = Some(failure.owner);
                Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
            }
            Err(failure) => {
                let inline_fallback = inline_reaper_fallback_allowed();
                if !inline_fallback {
                    *slot = Some(failure.owner);
                    return Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable);
                }
                failure.log_unavailable("deferred process cleanup");
                drop(slot);
                failure.owner.run_inline();
                if self.observer.is_complete() {
                    Ok(())
                } else {
                    Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
                }
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
    use std::sync::Arc;
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;

    use super::DeferredReaperOwner;
    use super::ReaperObserver;
    use super::ReaperOwner;
    use super::retry_cleanup_on_worker;
    use crate::execution::backend::HandoffFailure;
    use crate::execution::backend::HandoffFailureKind;
    use crate::execution::backend::Reaper;
    use crate::execution::contracts::RecoveryOwner;

    struct GateTeardown(Arc<(Mutex<bool>, Condvar)>);

    impl Drop for GateTeardown {
        fn drop(&mut self) {
            let (released, changed) = &*self.0;
            *released.lock().unwrap_or_else(|error| error.into_inner()) = true;
            changed.notify_all();
        }
    }

    #[test]
    fn test_public_retry_stays_nonblocking_but_worker_falls_back_inline() {
        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let cleanup_gate = Arc::clone(&gate);
        let cleanup_observer = observer.clone();
        let owner = Arc::new(DeferredReaperOwner::with_handoff(
            ReaperOwner::new(move || {
                started_tx.send(()).unwrap();
                let (released, changed) = &*cleanup_gate;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = changed.wait(released).unwrap();
                }
                cleanup_observer.complete();
            }),
            observer,
            Arc::new(|owner| {
                Err(HandoffFailure {
                    kind: HandoffFailureKind::StartFailed,
                    owner,
                    source: None,
                })
            }),
        ));

        assert_eq!(
            owner.retry_cleanup(),
            Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
        );
        assert!(
            started_rx.try_recv().is_err(),
            "public retry must not run inline cleanup"
        );

        let worker_owner = Arc::clone(&owner);
        let worker = std::thread::spawn(move || retry_cleanup_on_worker(&*worker_owner));
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            completed.try_recv().is_err(),
            "cleanup remains pending until owner releases"
        );
        let (released, changed) = &*gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
        assert_eq!(worker.join().unwrap(), Ok(()));
        completed.recv_timeout(Duration::from_secs(3)).unwrap();
    }

    #[test]
    fn test_private_worker_falls_back_after_actual_reaper_startup_failures() {
        let reaper = Arc::new(Reaper::with_startup_failures(2));
        let reaper_for_handoff = Arc::clone(&reaper);
        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let _teardown = GateTeardown(Arc::clone(&gate));
        let cleanup_gate = Arc::clone(&gate);
        let cleanup_observer = observer.clone();
        let owner = Arc::new(DeferredReaperOwner::with_handoff(
            ReaperOwner::new(move || {
                started_tx.send(()).unwrap();
                let (released, changed) = &*cleanup_gate;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = changed.wait(released).unwrap();
                }
                cleanup_observer.complete();
            }),
            observer.clone(),
            Arc::new(move |owner| reaper_for_handoff.handoff(owner)),
        ));

        assert_eq!(
            owner.retry_cleanup(),
            Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
        );
        assert!(started_rx.try_recv().is_err());
        let worker_owner = Arc::clone(&owner);
        let worker = std::thread::spawn(move || retry_cleanup_on_worker(&*worker_owner));
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            !observer.is_complete(),
            "inline fallback retains pending until actual cleanup"
        );

        let (released, changed) = &*gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
        assert_eq!(worker.join().unwrap(), Ok(()));
        completed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(owner.is_complete());
    }

    #[test]
    fn test_private_worker_retries_after_one_reaper_startup_failure() {
        let reaper = Arc::new(Reaper::with_startup_failures(1));
        let reaper_for_handoff = Arc::clone(&reaper);
        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let (cleanup_tx, cleanup_rx) = mpsc::sync_channel(1);
        let cleanup_observer = observer.clone();
        let owner = Arc::new(DeferredReaperOwner::with_handoff(
            ReaperOwner::new(move || {
                cleanup_observer.complete();
                cleanup_tx.send(()).unwrap();
            }),
            observer.clone(),
            Arc::new(move |owner| reaper_for_handoff.handoff(owner)),
        ));

        assert_eq!(
            owner.retry_cleanup(),
            Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
        );
        assert!(completed.try_recv().is_err());
        let worker_owner = Arc::clone(&owner);
        std::thread::spawn(move || retry_cleanup_on_worker(&*worker_owner))
            .join()
            .unwrap()
            .unwrap();
        cleanup_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        completed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(owner.is_complete(), "the healthy pool performs cleanup after retry");
    }

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
