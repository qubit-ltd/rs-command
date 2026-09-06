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
    join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
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
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
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
    pub(in crate::command_runner) fn cancel(&self) {
        self.cancellation.cancel(&self.join);
    }

    /// Joins the worker thread and returns its capture result.
    ///
    /// # Returns
    ///
    /// The worker result or a panic payload from the worker thread.
    pub(in crate::command_runner) fn join(self) -> thread::Result<Result<CapturedOutput, OutputCaptureError>> {
        self.join.join()
    }
}
