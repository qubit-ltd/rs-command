// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::TrySendError;
use std::sync::mpsc::sync_channel;
use std::thread;
use std::thread::JoinHandle;

use super::internal::ReaperOwner;

/// Represents the reaper state or contract used by managed command execution.
pub(crate) struct Reaper {
    /// Marks whether this handle routes through the shared replaceable pool.
    global: bool,
    /// Sender used by isolated test pools.
    sender: Option<SyncSender<ReaperOwner>>,
}

struct ReaperPool {
    sender: Option<SyncSender<ReaperOwner>>,
    workers: Vec<JoinHandle<()>>,
}

impl ReaperPool {
    fn shutdown(mut self) {
        drop(self.sender.take());
        for worker in self.workers {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HandoffFailureKind {
    Full,
    Disconnected,
    StartFailed,
}

pub(crate) struct HandoffFailure {
    pub(crate) kind: HandoffFailureKind,
    pub(crate) owner: ReaperOwner,
    pub(crate) source: Option<io::Error>,
}

impl std::fmt::Debug for HandoffFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandoffFailure")
            .field("kind", &self.kind)
            .field("raw_os_error", &self.source.as_ref().and_then(io::Error::raw_os_error))
            .finish_non_exhaustive()
    }
}

impl HandoffFailure {
    pub(crate) fn log_unavailable(&self, context: &str) {
        let os_error = self.source.as_ref().and_then(io::Error::raw_os_error);
        log::error!(
            "{context}: process reaper unavailable ({:?}, os_error={os_error:?})",
            self.kind
        );
    }
}

static REAPER_POOL: OnceLock<Mutex<Option<ReaperPool>>> = OnceLock::new();
static REAPER: OnceLock<Reaper> = OnceLock::new();

impl Reaper {
    #[cfg(test)]
    fn isolated(sender: SyncSender<ReaperOwner>) -> Self {
        Self {
            global: false,
            sender: Some(sender),
        }
    }

    /// Stores the initialize value used by this execution component.
    fn initialize() -> io::Result<ReaperPool> {
        const WORKERS: usize = 4;
        const CAPACITY: usize = 64;
        Self::initialize_with(WORKERS, CAPACITY, |index, receiver| {
            thread::Builder::new()
                .name(format!("qubit-command-reaper-{index}"))
                .spawn(move || worker_loop(receiver))
        })
    }

    fn initialize_with(
        workers_count: usize,
        capacity: usize,
        mut spawn_worker: impl FnMut(
            usize,
            std::sync::Arc<std::sync::Mutex<Receiver<ReaperOwner>>>,
        ) -> io::Result<JoinHandle<()>>,
    ) -> io::Result<ReaperPool> {
        let (sender, receiver) = sync_channel::<ReaperOwner>(capacity);
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
        let mut workers = Vec::with_capacity(workers_count);
        for index in 0..workers_count {
            match spawn_worker(index, std::sync::Arc::clone(&receiver)) {
                Ok(worker) => workers.push(worker),
                Err(error) => {
                    drop(sender);
                    for worker in workers {
                        let _ = worker.join();
                    }
                    return Err(error);
                }
            }
        }
        Ok(ReaperPool {
            sender: Some(sender),
            workers,
        })
    }

    /// Stores the global value used by this execution component.
    pub(crate) fn global() -> &'static Self {
        REAPER.get_or_init(|| Self {
            global: true,
            sender: None,
        })
    }

    /// Stores the handoff value used by this execution component.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    pub(crate) fn handoff(&self, owner: ReaperOwner) -> Result<(), HandoffFailure> {
        if !self.global {
            return send_owner(self.sender.as_ref().expect("isolated reaper sender"), owner);
        }
        let state = REAPER_POOL.get_or_init(|| Mutex::new(None));
        let mut pool = state.lock().unwrap_or_else(|error| error.into_inner());
        if pool.is_none() {
            match Self::initialize() {
                Ok(started) => *pool = Some(started),
                Err(source) => {
                    return Err(HandoffFailure {
                        kind: HandoffFailureKind::StartFailed,
                        owner,
                        source: Some(source),
                    });
                }
            }
        }
        let current = pool.as_ref().expect("pool was initialized");
        match send_owner(current.sender.as_ref().expect("pool sender"), owner) {
            Ok(()) => Ok(()),
            Err(failure) if failure.kind == HandoffFailureKind::Full => Err(failure),
            Err(failure) => {
                if let Some(stale) = pool.take() {
                    stale.shutdown();
                }
                match Self::initialize() {
                    Ok(replacement) => {
                        let result = send_owner(replacement.sender.as_ref().expect("pool sender"), failure.owner);
                        *pool = Some(replacement);
                        result
                    }
                    Err(source) => Err(HandoffFailure {
                        kind: HandoffFailureKind::StartFailed,
                        owner: failure.owner,
                        source: Some(source),
                    }),
                }
            }
        }
    }
}

fn send_owner(sender: &SyncSender<ReaperOwner>, owner: ReaperOwner) -> Result<(), HandoffFailure> {
    sender.try_send(owner).map_err(|error| match error {
        TrySendError::Full(owner) => HandoffFailure {
            kind: HandoffFailureKind::Full,
            owner,
            source: None,
        },
        TrySendError::Disconnected(owner) => HandoffFailure {
            kind: HandoffFailureKind::Disconnected,
            owner,
            source: None,
        },
    })
}

/// Receives one cleanup owner at a time and reaps it without holding the queue
/// lock.
///
/// # Parameters
/// * `receiver`: Shared receiver used by the reaper worker pool.
fn worker_loop(receiver: std::sync::Arc<std::sync::Mutex<Receiver<ReaperOwner>>>) {
    loop {
        let task = receiver.lock().unwrap_or_else(|error| error.into_inner()).recv();
        match task {
            Ok(task) => task.reap(),
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::sync_channel;
    use std::thread;
    use std::time::Duration;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::Reaper;
    use super::ReaperOwner;
    use crate::execution::backend::ReaperObserver;
    use crate::execution::backend::internal::DeferredReaperOwner;
    use crate::execution::contracts::FactPublishError;
    use crate::execution::contracts::FactSink;
    use crate::execution::contracts::RecoveryOwner;
    use crate::execution::contracts::RecoveryPublishError;
    use crate::execution::contracts::RecoveryPublishFailure;

    struct RecoverySink {
        retained: std::sync::Mutex<Option<Box<dyn RecoveryOwner>>>,
        reject: bool,
    }

    impl FactSink for RecoverySink {
        fn publish_outcome(
            &self,
            _outcome: OperationOutcome<crate::CommandOutput, crate::CommandError>,
        ) -> Result<(), FactPublishError> {
            unreachable!("reaper observer tests only use recovery owner retention")
        }

        fn confirm_input_release(&self) -> Result<(), FactPublishError> {
            unreachable!("reaper observer tests only use recovery owner retention")
        }

        fn publish_settlement(&self, _report: Arc<qubit_operation::SettlementReport>) -> Result<(), FactPublishError> {
            unreachable!("reaper observer tests only use recovery owner retention")
        }

        fn retain_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) -> Result<(), RecoveryPublishFailure> {
            if self.reject {
                Err(RecoveryPublishFailure {
                    reason: RecoveryPublishError::CapacityExceeded,
                    owner,
                })
            } else {
                *self.retained.lock().unwrap() = Some(owner);
                Ok(())
            }
        }
    }

    fn recovery_sink(reject: bool) -> Arc<RecoverySink> {
        Arc::new(RecoverySink {
            retained: std::sync::Mutex::new(None),
            reject,
        })
    }

    #[test]
    fn test_partial_reaper_startup_closes_started_workers_and_can_retry() {
        let exited = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&exited);
        let failed = Reaper::initialize_with(2, 1, move |index, receiver| {
            if index == 1 {
                return Err(io::Error::other("injected worker startup failure"));
            }
            let observed = Arc::clone(&observed);
            thread::Builder::new().spawn(move || {
                let _ = receiver.lock().unwrap_or_else(|error| error.into_inner()).recv();
                observed.fetch_add(1, Ordering::SeqCst);
            })
        });

        assert!(failed.is_err());
        assert_eq!(exited.load(Ordering::SeqCst), 1, "partial worker must be joined");

        let recovered = Reaper::initialize().expect("later startup attempt should succeed");
        recovered.shutdown();
    }

    #[test]
    fn test_global_reaper_initializes_once_and_accepts_cleanup_handoff() {
        let first = Reaper::global();
        let second = Reaper::global();
        assert!(std::ptr::eq(first, second));

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let owner = ReaperOwner::new(move || done_tx.send(()).unwrap());
        first.handoff(owner).expect("available queue should accept owner");
        done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("reaper worker should complete the handed-off cleanup");
    }

    #[test]
    fn test_reaper_observer_tracks_pending_completion_and_notifies_once() {
        let (observer, completed) = ReaperObserver::pair();
        assert!(!observer.is_pending());
        assert!(!observer.is_complete());
        assert!(!observer.recovery_was_transferred());

        let clone = observer.clone();
        clone.mark_pending();
        assert!(observer.is_pending(), "clones share the pending state");

        observer.complete();
        observer.complete();
        assert!(clone.is_complete());
        completed
            .try_recv()
            .expect("completion should publish one notification");
        assert!(completed.try_recv().is_err(), "completion is signaled once");
    }

    #[test]
    fn test_reaper_observer_transfers_recovery_owner_to_sink() {
        let sink = recovery_sink(false);
        let (observer, _completed) = ReaperObserver::pair_with_recovery_sink(Arc::clone(&sink) as Arc<dyn FactSink>);
        assert!(observer.recovery_sink().is_some());

        observer.retain_or_defer_reaper_owner(ReaperOwner::new(|| {}));

        assert!(observer.recovery_was_transferred());
        assert!(observer.take_deferred_recovery_owner().is_none());
        let retained = sink
            .retained
            .lock()
            .unwrap()
            .take()
            .expect("sink should retain the recovery owner");
        assert!(retained.summary().contains("reaper"));
    }

    #[test]
    fn test_reaper_observer_keeps_recovery_owner_when_sink_rejects_transfer() {
        let sink = recovery_sink(true);
        let (observer, _completed) = ReaperObserver::pair_with_recovery_sink(Arc::clone(&sink) as Arc<dyn FactSink>);

        observer.retain_or_defer_reaper_owner(ReaperOwner::new(|| {}));

        assert!(!observer.recovery_was_transferred());
        assert!(sink.retained.lock().unwrap().is_none());
        assert!(
            observer.take_deferred_recovery_owner().is_some(),
            "rejected owner should remain available to the backend"
        );
        assert!(observer.take_deferred_recovery_owner().is_none());
    }

    #[test]
    fn test_deferred_process_owner_reports_bounded_unknown_effect_while_cleanup_is_pending() {
        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let (cleanup_tx, cleanup_rx) = std::sync::mpsc::channel();
        let cleanup_observer = observer.clone();
        let owner = Arc::new(DeferredReaperOwner::new(
            ReaperOwner::new(move || {
                let _ = cleanup_rx.recv();
                cleanup_observer.complete();
            }),
            observer.clone(),
        ));
        owner.retry_cleanup().unwrap();
        let owner_for_operation = Arc::clone(&owner);
        let (evidence_tx, evidence_rx) = std::sync::mpsc::channel();
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                evidence_tx
                    .send((scope.id(), owner_for_operation.effect_evidence(scope.id())))
                    .unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();

        let (operation_id, evidence) = evidence_rx.recv().unwrap();
        assert_eq!(evidence.effects_total, 1);
        assert_eq!(evidence.effect_overflow, 0);
        assert_eq!(evidence.effects.len(), 1);
        let effect = &evidence.effects[0];
        assert_eq!(effect.operation_id, operation_id);
        assert_eq!(effect.resource_id, None);
        assert_eq!(effect.fact, qubit_operation::EffectFact::Unknown);
        assert_eq!(
            effect.description.as_str(),
            "managed process/helper cleanup is pending in reaper"
        );
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();

        cleanup_tx.send(()).unwrap();
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("reaper cleanup should publish completion");
        let completed_evidence = owner.effect_evidence(operation_id);
        assert!(completed_evidence.effects.is_empty());
        assert_eq!(completed_evidence.effects_total, 0);
        assert_eq!(completed_evidence.effect_overflow, 0);
    }

    #[test]
    fn test_bounded_reaper_executes_transferred_owner_before_completion_signal() {
        let reaper = Reaper::global();
        let released = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&released);
        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let cleanup_observer = observer.clone();
        let owner = ReaperOwner::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            cleanup_observer.complete();
        });
        reaper.handoff(owner).expect("reaper should accept owner");
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("cleanup should signal completion");
        assert_eq!(released.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_full_reaper_queue_returns_owner_for_retry_after_capacity_drains() {
        let (sender, primary) = sync_channel(1);
        let reaper = Reaper::isolated(sender);
        reaper.handoff(ReaperOwner::new(|| {})).unwrap();

        let completed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&completed);
        let owner = reaper
            .handoff(ReaperOwner::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            }))
            .expect_err("full bounded queue must return the still-owned task");
        assert_eq!(owner.kind, super::HandoffFailureKind::Full);
        let owner = owner.owner;
        assert_eq!(completed.load(Ordering::SeqCst), 0);

        primary
            .try_recv()
            .expect("the test should be able to release queue capacity")
            .run_inline();
        reaper
            .handoff(owner)
            .expect("returned owner should transfer after capacity drains");
        primary
            .try_recv()
            .expect("retried owner should be retained in the bounded queue")
            .run_inline();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_disconnected_reaper_queue_returns_cleanup_owner() {
        let (sender, primary) = sync_channel(1);
        drop(primary);
        let reaper = Reaper::isolated(sender);
        let completed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&completed);

        let owner = reaper
            .handoff(ReaperOwner::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            }))
            .expect_err("disconnected queue must return cleanup ownership");
        assert_eq!(owner.kind, super::HandoffFailureKind::Disconnected);
        let owner = owner.owner;
        assert_eq!(completed.load(Ordering::SeqCst), 0);
        owner.run_inline();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_idle_worker_can_take_task_while_two_workers_are_busy() {
        use std::sync::Condvar;
        use std::sync::Mutex;

        let (sender, receiver) = sync_channel(64);
        let receiver = Arc::new(Mutex::new(receiver));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (busy_tx, busy_rx) = std::sync::mpsc::channel();
        let (third_tx, third_rx) = std::sync::mpsc::channel();
        for _ in 0..3 {
            let receiver = Arc::clone(&receiver);
            std::thread::spawn(move || super::worker_loop(receiver));
        }
        for _ in 0..2 {
            let gate = Arc::clone(&gate);
            let busy_tx = busy_tx.clone();
            sender
                .try_send(ReaperOwner::new(move || {
                    busy_tx.send(()).unwrap();
                    let (released, changed) = &*gate;
                    let mut released = released.lock().unwrap();
                    while !*released {
                        released = changed.wait(released).unwrap();
                    }
                }))
                .unwrap();
        }
        busy_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        busy_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        sender
            .try_send(ReaperOwner::new(move || third_tx.send(()).unwrap()))
            .unwrap();
        third_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("third idle worker should receive queued work");
        let (released, changed) = &*gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
    }

    #[test]
    fn test_rejected_handoff_retains_cleanup_owner_until_single_completion() {
        let (sender, primary) = sync_channel(1);
        let reaper = Reaper::isolated(sender);
        reaper.handoff(ReaperOwner::new(|| {})).unwrap();

        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let cleanups = Arc::new(AtomicUsize::new(0));
        let cleanups_by_owner = Arc::clone(&cleanups);
        let cleanup_observer = observer.clone();
        let owner = reaper
            .handoff(ReaperOwner::new(move || {
                cleanups_by_owner.fetch_add(1, Ordering::SeqCst);
                cleanup_observer.complete();
            }))
            .expect_err("full queue must retain cleanup ownership with the caller");
        assert_eq!(owner.kind, super::HandoffFailureKind::Full);
        let owner = owner.owner;

        assert_eq!(cleanups.load(Ordering::SeqCst), 0);
        assert!(completed.try_recv().is_err());

        owner.run_inline();
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("inline cleanup should publish completion");
        observer.complete();

        assert_eq!(cleanups.load(Ordering::SeqCst), 1);
        assert!(completed.try_recv().is_err());
        drop(primary);
    }
}
