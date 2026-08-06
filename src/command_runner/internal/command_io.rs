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
    output_collector::{
        collect_output,
        collect_output_results,
        join_output_reader,
    },
    output_reader::OutputReader,
    stdin_pipe::join_stdin_writer,
    stdin_writer::StdinWriter,
};
use crate::{
    CommandError,
    CommandOutput,
};

/// Output and stdin helper threads for one running command.
#[must_use = "dropping command I/O detaches its helper threads"]
pub(in crate::command_runner) struct CommandIo {
    /// Reader thread draining stdout.
    stdout_reader: OutputReader,
    /// Reader thread draining stderr.
    stderr_reader: OutputReader,
    /// Optional writer thread feeding stdin.
    stdin_writer: StdinWriter,
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
        stdin_writer: StdinWriter,
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
                .is_none_or(std::thread::JoinHandle::is_finished)
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

    /// Collects only helpers that have already finished.
    ///
    /// Unfinished helpers are detached when this method consumes the bundle.
    /// This is used after process termination so an escaped descendant or a
    /// blocked tee destination cannot indefinitely delay a timeout or
    /// cancellation result. The returned output can therefore omit bytes from
    /// streams whose helpers were still active.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `status` - Process exit status.
    /// * `elapsed` - Callback that samples command duration without waiting for
    ///   unfinished helpers.
    ///
    /// # Returns
    ///
    /// Captured output from helpers that completed before the cleanup cutoff.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if a completed helper or elapsed-time sampling
    /// failed.
    pub(in crate::command_runner) fn collect_ready<F>(
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
        let stdout_result = if stdout_reader.is_finished() {
            join_output_reader(stdout_reader)
        } else {
            Ok(Default::default())
        };
        let stderr_result = if stderr_reader.is_finished() {
            join_output_reader(stderr_reader)
        } else {
            Ok(Default::default())
        };
        let stdin_result = match stdin_writer {
            Some(writer) if writer.is_finished() => {
                join_stdin_writer(command, Some(writer))
            }
            Some(_) | None => Ok(()),
        };
        collect_output_results(
            command,
            status,
            elapsed(),
            stdout_result,
            stderr_result,
            stdin_result,
        )
    }
}
