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

/// Owns the bounded cleanup queue and its worker threads.
pub(crate) struct ReaperPool {
    /// Accepts cleanup owners while the pool is active.
    pub(crate) sender: Option<SyncSender<ReaperOwner>>,
    /// Handles joined after the queue sender is closed.
    pub(crate) workers: Vec<JoinHandle<()>>,
}

impl ReaperPool {
    /// Closes the queue and waits for every worker to finish.
    pub(crate) fn shutdown(self) {
        drop(self);
    }
}

/// Defines worker shutdown when the pool is released.
impl Drop for ReaperPool {
    /// Closes the queue and joins each worker before releasing its handle.
    fn drop(&mut self) {
        drop(self.sender.take());
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
