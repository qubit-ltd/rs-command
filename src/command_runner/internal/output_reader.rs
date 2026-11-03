// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;
use std::thread;

use super::captured_output::CapturedOutput;
use super::io_cancellation::IoCancellation;
use super::output_capture_error::OutputCaptureError;
use super::total_output_budget::TotalOutputBudget;
use crate::OutputStream;

/// Output reader thread result type.
#[derive(Debug)]
pub(in crate::command_runner) struct OutputReader {
    /// Join handle for the worker that drains one child output pipe.
    join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
    /// Cancellation state used to interrupt the worker during cleanup.
    cancellation: IoCancellation,
    /// Budget whose bytes are transferred only after this reader joins.
    budget: Option<(Arc<TotalOutputBudget>, OutputStream)>,
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
        Self {
            join,
            cancellation,
            budget: None,
        }
    }

    /// Creates a budgeted output reader that transfers its bytes after join.
    #[inline]
    pub(in crate::command_runner) fn new_with_budget(
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
        cancellation: IoCancellation,
        budget: Arc<TotalOutputBudget>,
        stream: OutputStream,
    ) -> Self {
        Self {
            join,
            cancellation,
            budget: Some((budget, stream)),
        }
    }

    /// Returns whether the worker thread has stopped.
    ///
    /// # Returns
    ///
    /// `true` when the worker has stopped; otherwise, `false`.
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
    /// Returns the captured stream bytes and metadata after the worker joins.
    ///
    /// # Errors
    ///
    /// Returns an output-capture error when reading or teeing failed. A worker
    /// panic is mapped to a read error; its original panic payload is not
    /// returned.
    ///
    /// When a total budget is configured, retained bytes are moved from that
    /// budget after joining and replace the capture data in either the success
    /// or error result.
    #[must_use = "handle both thread and output capture failures"]
    #[inline]
    pub(in crate::command_runner) fn join(self) -> Result<CapturedOutput, OutputCaptureError> {
        let Self { join, budget, .. } = self;
        let result = match join.join() {
            Ok(result) => result,
            Err(_) => Err(OutputCaptureError::Read {
                source: std::io::Error::other("output reader thread panicked"),
                output: CapturedOutput {
                    complete: false,
                    ..CapturedOutput::default()
                },
            }),
        };
        match budget {
            Some((budget, stream)) => {
                let output = budget.take_captured_stream(stream);
                match result {
                    Ok(_) => Ok(output),
                    Err(error) => Err(replace_error_output(error, output)),
                }
            }
            None => result,
        }
    }
}

/// Replaces an error's captured data while preserving its failure reason.
///
/// The replacement carries the bytes retained by the shared budget after this
/// reader joins; the original `Read`, `Write`, or `ReadAfterWrite` cause and
/// its associated source/path information remain authoritative.
fn replace_error_output(error: OutputCaptureError, output: CapturedOutput) -> OutputCaptureError {
    match error {
        OutputCaptureError::Read { source, .. } => OutputCaptureError::Read { source, output },
        OutputCaptureError::Write { path, source, .. } => OutputCaptureError::Write { path, source, output },
        OutputCaptureError::ReadAfterWrite {
            path,
            write_source,
            read_source,
            ..
        } => OutputCaptureError::ReadAfterWrite {
            path,
            write_source,
            read_source,
            output,
        },
    }
}
