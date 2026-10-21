// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::thread;

use super::captured_output::CapturedOutput;
use super::io_cancellation::IoCancellation;
use super::output_capture_error::OutputCaptureError;

/// Output reader thread result type.
#[derive(Debug)]
pub(in crate::command_runner) struct OutputReader {
    /// Join handle for the worker that drains one child output pipe.
    join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
    /// Cancellation state used to interrupt the worker during cleanup.
    cancellation: IoCancellation,
}

impl OutputReader {
    /// Creates an output reader from its worker thread and cancellation handle.
    ///
    /// # Parameters
    ///
    /// * `join` - Worker thread collecting one output stream.
    /// * `cancellation` - Handle used to stop the worker during cleanup.
    ///
    /// # Returns
    ///
    /// An output reader owning the worker resources.
    #[inline]
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
        cancellation: IoCancellation,
    ) -> Self {
        Self { join, cancellation }
    }

    /// Returns whether the worker thread has stopped.
    #[must_use]
    #[inline]
    pub(in crate::command_runner) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    /// Requests cancellation of the worker thread.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if waking the worker fails, except when the wakeup
    /// channel is already closed, or if interrupting the worker fails.
    #[must_use = "handle output-reader cancellation failures"]
    #[inline]
    pub(in crate::command_runner) fn cancel(&self) -> std::io::Result<()> {
        self.cancellation.cancel(&self.join)
    }

    /// Joins the worker thread and returns its capture result.
    ///
    /// # Returns
    ///
    /// The inner result contains captured output or an output-capture error;
    /// the outer result contains that worker result or the worker's panic
    /// payload.
    ///
    /// # Errors
    ///
    /// Returns an outer error containing the panic payload if the worker
    /// thread panicked.
    #[must_use = "handle both thread and output capture failures"]
    #[inline]
    pub(in crate::command_runner) fn join(self) -> thread::Result<Result<CapturedOutput, OutputCaptureError>> {
        self.join.join()
    }
}
