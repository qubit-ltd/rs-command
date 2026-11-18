// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;

use super::DeferredReaperOwner;
use super::ReaperOwner;
use crate::execution::contracts::FactSink;
use crate::execution::contracts::RecoveryOwner;

/// Represents the reaper observer state or contract used by managed command
/// execution.
#[derive(Clone)]
pub(crate) struct ReaperObserver {
    /// Stores the pending value used by this execution component.
    pending: std::sync::Arc<AtomicBool>,
    /// Stores the completed value used by this execution component.
    completed: std::sync::Arc<AtomicBool>,
    /// Stores the recovery transferred value used by this execution component.
    recovery_transferred: std::sync::Arc<AtomicBool>,
    /// Stores the completion sender value used by this execution component.
    completion_sender: std::sync::Arc<std::sync::Mutex<Option<SyncSender<()>>>>,
    /// Stores the recovery sink value used by this execution component.
    recovery_sink: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dyn FactSink>>>>,
    /// Stores the deferred owner value used by this execution component.
    deferred_owner: std::sync::Arc<std::sync::Mutex<Option<Box<dyn RecoveryOwner>>>>,
}

impl ReaperObserver {
    /// Stores the pair value used by this execution component.
    pub(crate) fn pair() -> (Self, std::sync::mpsc::Receiver<()>) {
        let (sender, receiver) = sync_channel(1);
        (
            Self {
                pending: std::sync::Arc::new(AtomicBool::new(false)),
                completed: std::sync::Arc::new(AtomicBool::new(false)),
                recovery_transferred: std::sync::Arc::new(AtomicBool::new(false)),
                completion_sender: std::sync::Arc::new(std::sync::Mutex::new(Some(sender))),
                recovery_sink: std::sync::Arc::new(std::sync::Mutex::new(None)),
                deferred_owner: std::sync::Arc::new(std::sync::Mutex::new(None)),
            },
            receiver,
        )
    }

    /// Performs the pair with recovery sink step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `sink`: Supplies the sink value used by this operation.
    pub(crate) fn pair_with_recovery_sink(sink: std::sync::Arc<dyn FactSink>) -> (Self, std::sync::mpsc::Receiver<()>) {
        let (observer, receiver) = Self::pair();
        *observer.recovery_sink.lock().unwrap_or_else(|error| error.into_inner()) = Some(sink);
        (observer, receiver)
    }

    /// Stores the recovery sink value used by this execution component.
    pub(crate) fn recovery_sink(&self) -> Option<std::sync::Arc<dyn FactSink>> {
        self.recovery_sink
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Stores the defer recovery owner value used by this execution component.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    pub(crate) fn defer_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) {
        *self.deferred_owner.lock().unwrap_or_else(|error| error.into_inner()) = Some(owner);
    }

    /// Stores the retain or defer reaper owner value used by this execution
    /// component.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    pub(crate) fn retain_or_defer_reaper_owner(&self, owner: ReaperOwner) {
        let recovery_owner: Box<dyn RecoveryOwner> = Box::new(DeferredReaperOwner::new(owner, self.clone()));
        if let Some(sink) = self.recovery_sink() {
            match sink.retain_recovery_owner(recovery_owner) {
                Ok(()) => self.mark_recovery_transferred(),
                Err(failure) => self.defer_recovery_owner(failure.owner),
            }
        } else {
            self.defer_recovery_owner(recovery_owner);
        }
    }

    /// Performs the take deferred recovery owner step in the managed command
    /// execution lifecycle.
    pub(crate) fn take_deferred_recovery_owner(&self) -> Option<Box<dyn RecoveryOwner>> {
        self.deferred_owner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    /// Performs the is complete step in the managed command execution
    /// lifecycle.
    pub(crate) fn is_complete(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }

    /// Performs the mark recovery transferred step in the managed command
    /// execution lifecycle.
    pub(crate) fn mark_recovery_transferred(&self) {
        self.recovery_transferred.store(true, Ordering::Release);
    }

    /// Performs the recovery was transferred step in the managed command
    /// execution lifecycle.
    pub(crate) fn recovery_was_transferred(&self) -> bool {
        self.recovery_transferred.load(Ordering::Acquire)
    }

    /// Performs the mark pending step in the managed command execution
    /// lifecycle.
    pub(crate) fn mark_pending(&self) {
        self.pending.store(true, Ordering::Release);
    }

    /// Performs the is pending step in the managed command execution lifecycle.
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    /// Performs the complete step in the managed command execution lifecycle.
    pub(crate) fn complete(&self) {
        self.completed.store(true, Ordering::Release);
        if let Some(sender) = self
            .completion_sender
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = sender.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::SettlementReport;

    use super::ReaperObserver;
    use crate::CommandError;
    use crate::CommandOutput;
    use crate::execution::backend::ReaperOwner;
    use crate::execution::contracts::FactPublishError;
    use crate::execution::contracts::FactSink;
    use crate::execution::contracts::RecoveryOwner;
    use crate::execution::contracts::RecoveryPublishError;
    use crate::execution::contracts::RecoveryPublishFailure;

    struct RecoverySink {
        reject: bool,
        owners: Mutex<Vec<Box<dyn RecoveryOwner>>>,
    }

    impl FactSink for RecoverySink {
        fn publish_outcome(&self, _: OperationOutcome<CommandOutput, CommandError>) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn confirm_input_release(&self) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn publish_settlement(&self, _: Arc<SettlementReport>) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn retain_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) -> Result<(), RecoveryPublishFailure> {
            if self.reject {
                return Err(RecoveryPublishFailure {
                    reason: RecoveryPublishError::CapacityExceeded,
                    owner,
                });
            }
            self.owners.lock().unwrap().push(owner);
            Ok(())
        }
    }

    fn sink(reject: bool) -> Arc<RecoverySink> {
        Arc::new(RecoverySink {
            reject,
            owners: Mutex::new(Vec::new()),
        })
    }

    #[test]
    fn test_accepted_recovery_transfer_marks_handoff_and_completes_cleanup() {
        let sink = sink(false);
        let (observer, completed) = ReaperObserver::pair_with_recovery_sink(sink.clone());
        assert!(observer.recovery_sink().is_some());
        observer.mark_pending();
        assert!(observer.is_pending());
        let (cleaned_tx, cleaned_rx) = mpsc::channel();
        let cleanup_observer = observer.clone();

        observer.retain_or_defer_reaper_owner(ReaperOwner::new(move || {
            cleaned_tx.send(()).unwrap();
            cleanup_observer.complete();
        }));

        assert!(observer.recovery_was_transferred());
        assert!(observer.take_deferred_recovery_owner().is_none());
        let owner = sink
            .owners
            .lock()
            .unwrap()
            .pop()
            .expect("sink should own transferred recovery");
        assert_eq!(
            owner.summary(),
            "managed child and I/O helpers awaiting bounded reaper capacity"
        );
        owner.retry_cleanup().expect("owner should transfer to the reaper");
        cleaned_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("reaper should run cleanup");
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("observer completion should be signaled");
        assert!(observer.is_complete());
    }

    #[test]
    fn test_rejected_recovery_transfer_returns_owner_for_backend_retry() {
        let sink = sink(true);
        let (observer, completed) = ReaperObserver::pair_with_recovery_sink(sink);
        observer.mark_pending();
        let (cleaned_tx, cleaned_rx) = mpsc::channel();
        let cleanup_observer = observer.clone();
        observer.retain_or_defer_reaper_owner(ReaperOwner::new(move || {
            cleaned_tx.send(()).unwrap();
            cleanup_observer.complete();
        }));

        assert!(!observer.recovery_was_transferred());
        let owner = observer
            .take_deferred_recovery_owner()
            .expect("rejected owner should remain with the backend");
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let evidence = owner.effect_evidence(operation.id());
        assert_eq!(evidence.effects_total, 1);
        assert_eq!(evidence.effects[0].fact, qubit_operation::EffectFact::Unknown);
        assert_eq!(
            evidence.effects[0].description.as_str(),
            "managed process/helper cleanup awaits reaper handoff"
        );
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();

        owner
            .retry_cleanup()
            .expect("rejected owner should be retryable through the reaper");
        cleaned_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("reaper should run returned owner cleanup");
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("deferred owner should signal completion");
        assert!(observer.is_complete());
        assert!(observer.recovery_sink().is_some());
    }

    #[test]
    fn test_owner_is_deferred_when_no_recovery_sink_is_configured() {
        let (observer, completed) = ReaperObserver::pair();
        assert!(observer.recovery_sink().is_none());
        observer.mark_pending();
        let cleanup_observer = observer.clone();
        observer.retain_or_defer_reaper_owner(ReaperOwner::new(move || cleanup_observer.complete()));
        assert!(!observer.recovery_was_transferred());
        let owner = observer
            .take_deferred_recovery_owner()
            .expect("owner should remain available for caller handoff");
        assert_eq!(
            owner.summary(),
            "managed child and I/O helpers awaiting bounded reaper capacity"
        );
        owner
            .retry_cleanup()
            .expect("deferred owner should be handed to the reaper");
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("deferred owner should complete cleanup");
        assert!(observer.is_complete());
    }

    #[test]
    fn test_completion_is_idempotent_and_signals_waiter_once() {
        let (observer, completed) = ReaperObserver::pair();
        assert!(!observer.is_pending());
        assert!(!observer.is_complete());

        observer.complete();
        observer.complete();

        assert!(observer.is_complete());
        completed.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(completed.try_recv(), Err(mpsc::TryRecvError::Disconnected)));
    }

    #[test]
    fn test_concurrent_completion_signals_waiter_once() {
        const COMPLETERS: usize = 8;
        let (observer, completed) = ReaperObserver::pair();
        let barrier = Arc::new(std::sync::Barrier::new(COMPLETERS + 1));

        std::thread::scope(|scope| {
            for _ in 0..COMPLETERS {
                let observer = observer.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    observer.complete();
                });
            }
            barrier.wait();
        });

        assert!(observer.is_complete());
        completed
            .recv_timeout(Duration::from_secs(1))
            .expect("concurrent completion should notify the waiter");
        assert!(matches!(completed.try_recv(), Err(mpsc::TryRecvError::Disconnected)));
    }

    #[test]
    fn test_completion_after_receiver_drop_is_recorded_without_panicking() {
        let (observer, completed) = ReaperObserver::pair();
        drop(completed);

        observer.complete();
        observer.complete();

        assert!(observer.is_complete());
        assert!(!observer.is_pending());
    }

    #[test]
    fn test_recovery_sink_remains_readable_after_mutex_poisoning() {
        let sink = sink(false);
        let (observer, _completed) = ReaperObserver::pair_with_recovery_sink(Arc::clone(&sink) as Arc<dyn FactSink>);
        let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = observer.recovery_sink.lock().unwrap();
            panic!("poison recovery sink mutex for recovery test");
        }));

        assert!(poison_result.is_err());
        let recovered = observer
            .recovery_sink()
            .expect("poisoned sink lock should recover its value");
        assert!(Arc::ptr_eq(&recovered, &(sink as Arc<dyn FactSink>)));
    }

    #[test]
    fn test_deferred_owner_can_be_stored_and_taken_after_mutex_poisoning() {
        let (observer, completed) = ReaperObserver::pair();
        let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = observer.deferred_owner.lock().unwrap();
            panic!("poison deferred owner mutex for recovery test");
        }));
        assert!(poison_result.is_err());

        observer.mark_pending();
        let cleanup_observer = observer.clone();
        observer.retain_or_defer_reaper_owner(ReaperOwner::new(move || cleanup_observer.complete()));
        let owner = observer
            .take_deferred_recovery_owner()
            .expect("poisoned owner lock should retain deferred cleanup");
        assert_eq!(
            owner.summary(),
            "managed child and I/O helpers awaiting bounded reaper capacity"
        );
        owner.retry_cleanup().expect("recovered owner should be retryable");
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("recovered cleanup should signal completion");
        assert!(observer.is_complete());
    }

    #[test]
    fn test_completion_signals_waiter_after_sender_mutex_poisoning() {
        let (observer, completed) = ReaperObserver::pair();
        let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = observer.completion_sender.lock().unwrap();
            panic!("poison completion sender mutex for recovery test");
        }));
        assert!(poison_result.is_err());

        observer.complete();

        assert!(observer.is_complete());
        completed
            .recv_timeout(Duration::from_secs(1))
            .expect("complete should signal through the recovered sender");
        assert!(matches!(completed.try_recv(), Err(mpsc::TryRecvError::Disconnected)));
    }
}
