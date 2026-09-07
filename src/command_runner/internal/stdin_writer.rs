// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;
use std::thread;

use super::io_cancellation::IoCancellation;

/// Stdin writer thread and its cancellation state.
#[derive(Debug)]
pub(in crate::command_runner) struct StdinWriter {
    /// Join handle for the worker that writes the configured stdin bytes.
    pub(in crate::command_runner) join: thread::JoinHandle<io::Result<()>>,
    /// Cancellation state used to interrupt the worker during cleanup.
    pub(in crate::command_runner) cancellation: IoCancellation,
}

impl StdinWriter {
    /// Creates a stdin writer from its worker thread and cancellation handle.
    ///
    /// # Parameters
    ///
    /// * `join` - Worker thread writing stdin bytes.
    /// * `cancellation` - Handle used to stop the worker during cleanup.
    ///
    /// # Returns
    ///
    /// A stdin writer owning the worker resources.
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<io::Result<()>>,
        cancellation: IoCancellation,
    ) -> Self {
        Self { join, cancellation }
    }

    /// Returns whether the worker thread has stopped.
    #[must_use]
    pub(in crate::command_runner) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    /// Requests cancellation of the worker thread.
    #[must_use = "handle stdin-writer cancellation failures"]
    pub(in crate::command_runner) fn cancel(&self) -> io::Result<()> {
        self.cancellation.cancel(&self.join)
    }

    /// Joins the worker thread and returns its write result.
    ///
    /// # Returns
    ///
    /// The worker result or a panic payload from the worker thread.
    #[must_use = "handle both thread and stdin write failures"]
    pub(in crate::command_runner) fn join(self) -> thread::Result<io::Result<()>> {
        self.join.join()
    }
}

/// Optional stdin writer helper.
///
/// `None` means the command inherits or discards stdin without a writer
/// thread; `Some` owns the worker that must be cancelled and joined.
pub(in crate::command_runner) type OptionalStdinWriter = Option<StdinWriter>;
