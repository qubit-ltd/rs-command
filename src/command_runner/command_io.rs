/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::time::Duration;

use super::{
    output_collector::collect_output,
    output_reader::OutputReader,
    stdin_writer::StdinWriter,
};
use crate::{
    CommandError,
    CommandOutput,
};

/// Output and stdin helper threads for one running command.
pub(crate) struct CommandIo {
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
    pub(crate) fn new(
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

    /// Collects output from all helper threads.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `status` - Process exit status.
    /// * `elapsed` - Observed command duration.
    ///
    /// # Returns
    ///
    /// Captured command output.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if stream collection or stdin writing fails.
    pub(crate) fn collect(
        self,
        command: &str,
        status: std::process::ExitStatus,
        elapsed: Duration,
    ) -> Result<CommandOutput, CommandError> {
        collect_output(
            command,
            status,
            elapsed,
            self.stdout_reader,
            self.stderr_reader,
            self.stdin_writer,
        )
    }
}
