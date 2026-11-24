// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::mpsc::SyncSender;
use std::thread::JoinHandle;

use super::ReaperOwner;

/// Owns the bounded reaper queue and the workers that drain it.
pub(crate) struct ReaperPool {
    /// Sends cleanup owners to a worker while the pool is active.
    pub(crate) sender: Option<SyncSender<ReaperOwner>>,
    /// Joins every worker after closing the queue sender.
    pub(crate) workers: Vec<JoinHandle<()>>,
}

impl ReaperPool {
    /// Closes the queue and joins all workers before returning.
    pub(crate) fn shutdown(self) {
        drop(self);
    }
}

/// Closes the queue and joins each worker when the pool is released.
impl Drop for ReaperPool {
    /// Closes the queue and joins each worker before releasing the pool.
    fn drop(&mut self) {
        drop(self.sender.take());
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
