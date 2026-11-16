// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::OnceLock;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;
use std::thread;

use super::internal::ReaperOwner;

/// Represents the reaper state or contract used by managed command execution.
pub(crate) struct Reaper {
    /// Stores the sender value used by this execution component.
    sender: SyncSender<ReaperOwner>,
    /// Stores the overflow sender value used by this execution component.
    overflow_sender: SyncSender<ReaperOwner>,
}

static REAPER: OnceLock<Result<Reaper, String>> = OnceLock::new();

impl Reaper {
    /// Stores the initialize value used by this execution component.
    fn initialize() -> io::Result<Self> {
        const WORKERS: usize = 4;
        const CAPACITY: usize = 32;
        let (sender, receiver) = sync_channel::<ReaperOwner>(CAPACITY);
        let (overflow_sender, overflow_receiver) = sync_channel::<ReaperOwner>(CAPACITY);
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
        let overflow_receiver = std::sync::Arc::new(std::sync::Mutex::new(overflow_receiver));
        for index in 0..WORKERS {
            let (receiver, name) = if index < WORKERS / 2 {
                (std::sync::Arc::clone(&receiver), "primary")
            } else {
                (std::sync::Arc::clone(&overflow_receiver), "overflow")
            };
            thread::Builder::new()
                .name(format!("qubit-command-reaper-{name}-{index}"))
                .spawn(move || {
                    loop {
                        let task = receiver.lock().unwrap_or_else(|error| error.into_inner()).recv();
                        match task {
                            Ok(task) => task.reap(),
                            Err(_) => break,
                        }
                    }
                })?;
        }
        Ok(Self {
            sender,
            overflow_sender,
        })
    }

    /// Stores the global value used by this execution component.
    pub(crate) fn global() -> io::Result<&'static Self> {
        match REAPER.get_or_init(|| Self::initialize().map_err(|error| error.to_string())) {
            Ok(reaper) => Ok(reaper),
            Err(error) => Err(io::Error::other(error.clone())),
        }
    }

    /// Stores the handoff value used by this execution component.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    pub(crate) fn handoff(&self, owner: ReaperOwner) -> Result<(), ReaperOwner> {
        match self.sender.try_send(owner) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(owner) | TrySendError::Disconnected(owner)) => {
                match self.overflow_sender.try_send(owner) {
                    Ok(()) => Ok(()),
                    Err(TrySendError::Full(owner) | TrySendError::Disconnected(owner)) => Err(owner),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::DeferredReaperOwner;
    use super::Reaper;
    use super::ReaperObserver;
    use super::ReaperOwner;
    use crate::execution::contracts::RecoveryOwner;

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
        let reaper = Reaper::global().expect("reaper workers should start");
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
    fn test_full_primary_queue_returns_owner_to_bounded_overflow_queue_without_blocking() {
        let (sender, primary) = sync_channel(1);
        let (overflow_sender, overflow) = sync_channel(1);
        let reaper = Reaper {
            sender,
            overflow_sender,
        };
        reaper
            .handoff(ReaperOwner::new(|| {}))
            .expect("primary queue should accept first owner");
        let completed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&completed);
        reaper
            .handoff(ReaperOwner::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("overflow queue should accept owner when primary is full");
        overflow
            .try_recv()
            .expect("owner should be retained by overflow queue")
            .run_inline();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        drop(primary);
    }

    #[test]
    fn test_both_full_queues_return_owner_for_retry_after_capacity_drains() {
        let (sender, primary) = sync_channel(1);
        let (overflow_sender, _overflow) = sync_channel(1);
        let reaper = Reaper {
            sender,
            overflow_sender,
        };
        reaper.handoff(ReaperOwner::new(|| {})).unwrap();
        reaper.handoff(ReaperOwner::new(|| {})).unwrap();

        let completed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&completed);
        let owner = reaper
            .handoff(ReaperOwner::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            }))
            .expect_err("full bounded queues must return the still-owned task");
        assert_eq!(completed.load(Ordering::SeqCst), 0);

        primary
            .try_recv()
            .expect("the test should be able to release primary capacity")
            .run_inline();
        reaper
            .handoff(owner)
            .expect("returned owner should transfer after capacity drains");
        primary
            .try_recv()
            .expect("retried owner should be retained in a bounded queue")
            .run_inline();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }
}
