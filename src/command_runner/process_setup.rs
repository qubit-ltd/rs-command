/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
};

use crate::command_stdin::CommandStdin;
use crate::{Command, CommandError, OutputStream};

/// Fully prepared standard-library command plus runner-side I/O resources.
pub(crate) struct PreparedCommand {
    /// Human-readable command text for logs and diagnostics.
    pub(crate) command_text: String,
    /// Process command ready to spawn.
    pub(crate) process_command: ProcessCommand,
    /// Bytes to write to stdin after spawning, if configured.
    pub(crate) stdin_bytes: Option<Vec<u8>>,
    /// Open tee file for stdout.
    pub(crate) stdout_file: Option<File>,
    /// Open tee file for stderr.
    pub(crate) stderr_file: Option<File>,
    /// Diagnostic path for stdout tee writes.
    pub(crate) stdout_file_path: Option<PathBuf>,
    /// Diagnostic path for stderr tee writes.
    pub(crate) stderr_file_path: Option<PathBuf>,
}

impl PreparedCommand {
    /// Creates the process command and all pre-spawn I/O resources.
    pub(crate) fn prepare(
        command: Command,
        default_working_directory: Option<&Path>,
        stdout_file_path: Option<&Path>,
        stderr_file_path: Option<&Path>,
    ) -> Result<Self, CommandError> {
        let command_text = command.display_command();
        let mut process_command = ProcessCommand::new(command.program());
        process_command.args(command.arguments());
        process_command.stdout(Stdio::piped());
        process_command.stderr(Stdio::piped());

        if let Some(working_directory) = command
            .working_directory_override()
            .or(default_working_directory)
        {
            process_command.current_dir(working_directory);
        }

        configure_environment(&command, &mut process_command);
        let stdin = command.into_stdin_configuration();
        let stdin_bytes = configure_stdin(&command_text, stdin, &mut process_command)?;
        let stdout_file = open_output_file(&command_text, OutputStream::Stdout, stdout_file_path)?;
        let stderr_file = open_output_file(&command_text, OutputStream::Stderr, stderr_file_path)?;

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

/// Configures stdin for a process command.
fn configure_stdin(
    command_text: &str,
    stdin: CommandStdin,
    process_command: &mut ProcessCommand,
) -> Result<Option<Vec<u8>>, CommandError> {
    match stdin {
        CommandStdin::Null => {
            process_command.stdin(Stdio::null());
            Ok(None)
        }
        CommandStdin::Inherit => {
            process_command.stdin(Stdio::inherit());
            Ok(None)
        }
        CommandStdin::Bytes(bytes) => {
            process_command.stdin(Stdio::piped());
            Ok(Some(bytes))
        }
        CommandStdin::File(path) => match File::open(&path) {
            Ok(file) => {
                process_command.stdin(Stdio::from(file));
                Ok(None)
            }
            Err(source) => Err(CommandError::OpenInputFailed {
                command: command_text.to_owned(),
                path,
                source,
            }),
        },
    }
}

/// Configures environment variables for a process command.
fn configure_environment(command: &Command, process_command: &mut ProcessCommand) {
    if command.clears_environment() {
        process_command.env_clear();
    }
    for key in command.removed_environment() {
        process_command.env_remove(key);
    }
    for (key, value) in command.environment() {
        process_command.env(key, value);
    }
}

/// Opens an output tee file before spawning the child.
fn open_output_file(
    command: &str,
    stream: OutputStream,
    path: Option<&Path>,
) -> Result<Option<File>, CommandError> {
    match path {
        Some(path) => {
            File::create(path)
                .map(Some)
                .map_err(|source| CommandError::OpenOutputFailed {
                    command: command.to_owned(),
                    stream,
                    path: path.to_path_buf(),
                    source,
                })
        }
        None => Ok(None),
    }
}
