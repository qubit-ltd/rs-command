// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::time::Duration;

use qubit_clock::TimeError;

use super::{
    output_capture_error::OutputCaptureError,
    output_collector::{
        collect_output,
        collect_output_results,
        join_output_reader,
    },
    output_reader::OutputReader,
    stdin_pipe::join_stdin_writer,
    stdin_writer::OptionalStdinWriter,
};
use crate::{
    CommandError,
    CommandOutput,
    OutputStream,
};

/// Output and stdin helper threads for one running command.
#[must_use = "command I/O owns helper threads that must be collected"]
pub(in crate::command_runner) struct CommandIo {
    /// Reader thread draining stdout.
    stdout_reader: OutputReader,
    /// Reader thread draining stderr.
    stderr_reader: OutputReader,
    /// Optional writer thread feeding stdin.
    stdin_writer: OptionalStdinWriter,
}

impl CommandIo {
    /// Creates a command I/O helper bundle.
    ///
    /// # Parameters
    ///
    /// * `stdout_reader` - Reader thread draining stdout.
    /// * `stderr_reader` - Reader thread draining stderr.
    /// * `stdin_writer` - Optional writer thread feeding stdin.
    ///
    /// # Returns
    ///
    /// I/O helper bundle consumed when output is collected or drained.
    #[inline]
    pub(in crate::command_runner) fn new(
        stdout_reader: OutputReader,
        stderr_reader: OutputReader,
        stdin_writer: OptionalStdinWriter,
    ) -> Self {
        Self {
            stdout_reader,
            stderr_reader,
            stdin_writer,
        }
    }

    /// Returns whether all helper threads have finished.
    ///
    /// # Returns
    ///
    /// `true` when stdout, stderr, and optional stdin helpers can be joined
    /// without blocking.
    #[must_use]
    #[inline]
    pub(in crate::command_runner) fn is_finished(&self) -> bool {
        self.stdout_reader.is_finished()
            && self.stderr_reader.is_finished()
            && self
                .stdin_writer
                .as_ref()
                .is_none_or(|writer| writer.is_finished())
    }

    /// Collects output from all helper threads.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `status` - Process exit status.
    /// * `elapsed` - Callback that samples command duration after all helper
    ///   threads have finished.
    ///
    /// # Returns
    ///
    /// Captured command output.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if stream collection or stdin writing fails.
    #[inline(always)]
    pub(in crate::command_runner) fn collect<F>(
        self,
        command: &str,
        status: std::process::ExitStatus,
        elapsed: F,
    ) -> Result<CommandOutput, CommandError>
    where
        F: FnOnce() -> Result<Duration, TimeError>,
    {
        collect_output(
            command,
            status,
            elapsed,
            self.stdout_reader,
            self.stderr_reader,
            self.stdin_writer,
        )
    }

    /// Cancels and joins every helper after process termination.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `status` - Process exit status.
    /// * `elapsed` - Callback that samples command duration after helpers have
    ///   been cancelled and joined.
    ///
    /// # Returns
    ///
    /// Captured output, including completeness metadata for interrupted
    /// streams.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if a completed helper or elapsed-time sampling
    /// failed.
    pub(in crate::command_runner) fn cancel_and_collect<F>(
        self,
        command: &str,
        status: std::process::ExitStatus,
        elapsed: F,
    ) -> Result<CommandOutput, CommandError>
    where
        F: FnOnce() -> Result<Duration, TimeError>,
    {
        let Self {
            stdout_reader,
            stderr_reader,
            stdin_writer,
        } = self;
        stdout_reader.cancel();
        stderr_reader.cancel();
        if let Some(writer) = stdin_writer.as_ref() {
            writer.cancel();
        }
        let stdout_result = join_output_reader(stdout_reader);
        let stderr_result = join_output_reader(stderr_reader);
        let stdin_result = join_stdin_writer(command, stdin_writer);
        collect_output_results(
            command,
            status,
            elapsed(),
            stdout_result,
            stderr_result,
            stdin_result,
        )
    }

    /// Cancels and joins every helper without process status.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    ///
    /// # Returns
    ///
    /// Ok when helpers are joined, or the first helper failure in
    /// stdout/stderr/stdin order after all joins complete.
    pub(in crate::command_runner) fn cancel_and_join(
        self,
        command: &str,
    ) -> Result<(), CommandError> {
        let Self {
            stdout_reader,
            stderr_reader,
            stdin_writer,
        } = self;
        stdout_reader.cancel();
        stderr_reader.cancel();
        if let Some(writer) = stdin_writer.as_ref() {
            writer.cancel();
        }
        let stdout_result = join_output_reader(stdout_reader);
        let stderr_result = join_output_reader(stderr_reader);
        let stdin_result = join_stdin_writer(command, stdin_writer);

        let stdout_error = match stdout_result {
            Ok(_) => None,
            Err(OutputCaptureError::Read { source, .. }) => {
                Some(CommandError::ReadOutputFailed {
                    command: command.to_owned(),
                    stream: OutputStream::Stdout,
                    source,
                    output: None,
                })
            }
            Err(OutputCaptureError::Write { path, source, .. }) => {
                Some(CommandError::WriteOutputFailed {
                    command: command.to_owned(),
                    stream: OutputStream::Stdout,
                    path,
                    source,
                    output: None,
                })
            }
        };
        let stderr_error = match stderr_result {
            Ok(_) => None,
            Err(OutputCaptureError::Read { source, .. }) => {
                Some(CommandError::ReadOutputFailed {
                    command: command.to_owned(),
                    stream: OutputStream::Stderr,
                    source,
                    output: None,
                })
            }
            Err(OutputCaptureError::Write { path, source, .. }) => {
                Some(CommandError::WriteOutputFailed {
                    command: command.to_owned(),
                    stream: OutputStream::Stderr,
                    path,
                    source,
                    output: None,
                })
            }
        };
        if let Some(error) = stdout_error {
            return Err(error);
        }
        if let Some(error) = stderr_error {
            return Err(error);
        }
        stdin_result
    }
}
