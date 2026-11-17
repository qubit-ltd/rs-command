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
