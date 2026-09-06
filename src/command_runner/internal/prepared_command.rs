// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use qubit_redact::RedactionPolicy;

use super::io_files::IoFiles;
use super::process_setup::configure_environment;
use crate::Command;
use crate::CommandError;

/// Non-destructively prepared command awaiting I/O commit.
pub(in crate::command_runner) struct PreparedCommand {
    /// Human-readable command text for logs and diagnostics.
    pub(in crate::command_runner) command_text: String,
    /// Process command ready to spawn.
    pub(in crate::command_runner) process_command: ProcessCommand,
    /// Bytes to write to stdin after spawning, if configured.
    pub(in crate::command_runner) stdin_bytes: Option<Vec<u8>>,
    /// Open stdin file retained through the cancellation linearization point.
    pub(in crate::command_runner) stdin_path: Option<PathBuf>,
    /// Open stdin file retained through the cancellation linearization point.
    pub(in crate::command_runner) stdin_file: Option<File>,
    /// Open tee file for stdout.
    pub(in crate::command_runner) stdout_file: Option<File>,
    /// Open tee file for stderr.
    pub(in crate::command_runner) stderr_file: Option<File>,
    /// Diagnostic path for stdout tee writes.
    pub(in crate::command_runner) stdout_file_path: Option<PathBuf>,
    /// Diagnostic path for stderr tee writes.
    pub(in crate::command_runner) stderr_file_path: Option<PathBuf>,
}

impl PreparedCommand {
    /// Creates the process command and performs non-destructive I/O checks.
    ///
    /// # Parameters
    ///
    /// * `command` - Structured command to prepare.
    /// * `redaction_policy` - Immutable policy used to build diagnostic command
    ///   text.
    /// * `default_working_directory` - Runner working directory used when the
    ///   command has no override.
    /// * `stdout_file_path` - Optional stdout tee path.
    /// * `stderr_file_path` - Optional stderr tee path.
    ///
    /// # Returns
    ///
    /// Process builder, redacted diagnostics, and resources awaiting commit.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when an I/O file cannot be opened or inspected,
    /// or when configured input and output files conflict.
    pub(in crate::command_runner) fn prepare(
        command: Command,
        redaction_policy: &RedactionPolicy,
        default_working_directory: Option<&Path>,
        stdout_file_path: Option<&Path>,
        stderr_file_path: Option<&Path>,
    ) -> Result<Self, CommandError> {
        let command_text = command.display_command(redaction_policy);
        let mut process_command = ProcessCommand::new(command.program());
        process_command.args(command.arguments());
        process_command.stdout(std::process::Stdio::piped());
        process_command.stderr(std::process::Stdio::piped());

        if let Some(working_directory) = command.working_directory_override().or(default_working_directory) {
            process_command.current_dir(working_directory);
        }

        configure_environment(&command, &mut process_command);
        let IoFiles {
            stdin_path,
            stdin_bytes,
            stdin_file,
            stdout_file: _,
            stderr_file: _,
        } = IoFiles::prepare(
            &command_text,
            command.into_stdin_configuration(),
            stdout_file_path,
            stderr_file_path,
            &mut process_command,
        )?;

        Ok(Self {
            command_text,
            process_command,
            stdin_bytes,
            stdin_path,
            stdin_file,
            stdout_file: None,
            stderr_file: None,
            stdout_file_path: stdout_file_path.map(Path::to_path_buf),
            stderr_file_path: stderr_file_path.map(Path::to_path_buf),
        })
    }

    /// Commits tee file creation and attaches the validated stdin file.
    pub(in crate::command_runner) fn commit(mut self) -> Result<Self, CommandError> {
        let mut io_files = IoFiles {
            stdin_bytes: self.stdin_bytes.take(),
            stdin_path: self.stdin_path.take(),
            stdin_file: self.stdin_file.take(),
            stdout_file: self.stdout_file.take(),
            stderr_file: self.stderr_file.take(),
        };
        io_files.commit(
            &self.command_text,
            self.stdout_file_path.as_deref(),
            self.stderr_file_path.as_deref(),
            &mut self.process_command,
        )?;
        self.stdin_bytes = io_files.stdin_bytes;
        self.stdin_path = io_files.stdin_path;
        self.stdin_file = io_files.stdin_file;
        self.stdout_file = io_files.stdout_file;
        self.stderr_file = io_files.stderr_file;
        Ok(self)
    }
}
