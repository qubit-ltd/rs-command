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

use super::RecoveryItem;

/// Bounded submission endpoint for executor recovery work.
///
/// The outstanding budget includes queued items and items already held by the
/// worker. It allows the channel capacity plus one active item.
#[derive(Clone)]
pub(crate) struct RecoverySender {
    /// Channel sender consumed by the recovery worker.
    sender: SyncSender<RecoveryItem>,
    /// Shared count of queued and active recovery items.
    outstanding: Arc<AtomicUsize>,
    /// Maximum outstanding count, including one active item.
    capacity: usize,
}

impl RecoverySender {
    /// Creates a recovery sender with a shared outstanding-item budget.
    ///
    /// # Parameters
    /// * `sender`: Channel sender consumed by the recovery worker.
    /// * `capacity`: Queue capacity; the outstanding budget is this value plus
    ///   one active item.
    pub(crate) fn new(sender: SyncSender<RecoveryItem>, capacity: usize) -> Self {
        Self {
            sender,
            outstanding: Arc::new(AtomicUsize::new(0)),
            capacity: capacity.max(1).saturating_add(1),
        }
    }

    /// Shares the outstanding counter with the recovery worker.
    pub(crate) fn outstanding(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.outstanding)
    }

    /// Tries to enqueue an item while enforcing the combined queue and worker
    /// budget.
    ///
    /// Returns the original item if the budget is full or the receiver has
    /// disconnected.
    ///
    /// # Parameters
    /// * `item`: Recovery owner and operation state to enqueue.
    pub(crate) fn try_send(&self, item: RecoveryItem) -> Result<(), TrySendError<RecoveryItem>> {
        let mut count = self.outstanding.load(Ordering::Acquire);
        loop {
            if count >= self.capacity {
                return Err(TrySendError::Full(item));
            }
            match self
                .outstanding
                .compare_exchange_weak(count, count + 1, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(actual) => count = actual,
            }
        }
        self.sender.try_send(item).map_err(|error| {
            self.outstanding.fetch_sub(1, Ordering::AcqRel);
            error
        })
    }

    /// Sends a test item through the channel without the production budget
    /// check.
    ///
    /// # Parameters
    /// * `item`: Recovery item to send.
    #[cfg(test)]
    pub(crate) fn send(&self, item: RecoveryItem) -> Result<(), std::sync::mpsc::SendError<RecoveryItem>> {
        self.outstanding.fetch_add(1, Ordering::AcqRel);
        self.sender.send(item).map_err(|error| {
            self.outstanding.fetch_sub(1, Ordering::AcqRel);
            error
        })
    }
}
