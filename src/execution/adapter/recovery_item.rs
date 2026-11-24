// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::TrySendError;

use qubit_operation::OperationId;

use super::CommandControl;
use super::OperationObserverSlot;
use crate::execution::contracts::RecoveryOwner;

/// Represents the recovery item state or contract used by managed command
/// execution.
pub(crate) struct RecoveryItem {
    /// Stores the owner value used by this execution component.
    pub owner: Box<dyn RecoveryOwner>,
    /// Stores the control value used by this execution component.
    pub control: Arc<CommandControl>,
    /// Stores the id value used by this execution component.
    pub id: OperationId,
    /// Stores the observer value used by this execution component.
    pub observer: Arc<OperationObserverSlot>,
}

/// Represents the recovery sender state or contract used by managed command
/// execution.
#[derive(Clone)]
pub(crate) struct RecoverySender {
    sender: SyncSender<RecoveryItem>,
    outstanding: Arc<AtomicUsize>,
    capacity: usize,
}

impl RecoverySender {
    pub(crate) fn new(sender: SyncSender<RecoveryItem>, capacity: usize) -> Self {
        Self {
            sender,
            outstanding: Arc::new(AtomicUsize::new(0)),
            capacity: capacity.max(1),
        }
    }

    pub(crate) fn outstanding(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.outstanding)
    }

    pub(crate) fn try_send(&self, item: RecoveryItem) -> Result<(), TrySendError<RecoveryItem>> {
        let mut count = self.outstanding.load(Ordering::Acquire);
        loop {
            if count >= self.capacity {
                return Err(TrySendError::Full(item));
            }
            match self.outstanding.compare_exchange_weak(
                count,
                count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => count = actual,
            }
        }
        self.sender.try_send(item).map_err(|error| {
            self.outstanding.fetch_sub(1, Ordering::AcqRel);
            error
        })
    }

    #[cfg(test)]
    pub(crate) fn send(
        &self,
        item: RecoveryItem,
    ) -> Result<(), std::sync::mpsc::SendError<RecoveryItem>> {
        self.outstanding.fetch_add(1, Ordering::AcqRel);
        self.sender.send(item).map_err(|error| {
            self.outstanding.fetch_sub(1, Ordering::AcqRel);
            error
        })
    }
}
