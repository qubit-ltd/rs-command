// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    fs::File,
    path::{
        Path,
        PathBuf,
    },
    process::Command as ProcessCommand,
};

use qubit_sanitize::FieldSanitizer;

use super::io_files::IoFiles;
use super::process_setup::configure_environment;
use crate::{
    Command,
    CommandError,
};

/// Fully prepared standard-library command plus runner-side I/O resources.
pub(in crate::command_runner) struct PreparedCommand {
    /// Human-readable command text for logs and diagnostics.
    pub(in crate::command_runner) command_text: String,
    /// Process command ready to spawn.
    pub(in crate::command_runner) process_command: ProcessCommand,
    /// Bytes to write to stdin after spawning, if configured.
    pub(in crate::command_runner) stdin_bytes: Option<Vec<u8>>,
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
    /// Creates the process command and all pre-spawn I/O resources.
    ///
    /// # Parameters
    ///
    /// * `command` - Structured command to prepare.
    /// * `field_sanitizer` - Sanitizer used to build diagnostic command text.
    /// * `default_working_directory` - Runner working directory used when the
    ///   command has no override.
    /// * `stdout_file_path` - Optional stdout tee path.
    /// * `stderr_file_path` - Optional stderr tee path.
    ///
    /// # Returns
    ///
    /// Process builder, sanitized diagnostics, and validated I/O resources.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when an I/O file cannot be opened or inspected,
    /// or when configured input and output files conflict.
    pub(in crate::command_runner) fn prepare(
        command: Command,
        field_sanitizer: &FieldSanitizer,
        default_working_directory: Option<&Path>,
        stdout_file_path: Option<&Path>,
        stderr_file_path: Option<&Path>,
    ) -> Result<Self, CommandError> {
        let command_text = command.display_command(field_sanitizer);
        let mut process_command = ProcessCommand::new(command.program());
        process_command.args(command.arguments());
        process_command.stdout(std::process::Stdio::piped());
        process_command.stderr(std::process::Stdio::piped());

        if let Some(working_directory) = command
            .working_directory_override()
            .or(default_working_directory)
        {
            process_command.current_dir(working_directory);
        }

        configure_environment(&command, &mut process_command);
        let IoFiles {
            stdin_bytes,
            stdout_file,
            stderr_file,
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
            stdout_file,
            stderr_file,
            stdout_file_path: stdout_file_path.map(Path::to_path_buf),
            stderr_file_path: stderr_file_path.map(Path::to_path_buf),
        })
    }
}
