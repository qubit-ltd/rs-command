// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Prepares command I/O files without truncating conflicting paths.

use std::{
    fs::{
        self,
        File,
        OpenOptions,
    },
    io,
    path::{
        Component,
        Path,
        PathBuf,
    },
    process::{
        Command as ProcessCommand,
        Stdio,
    },
};

use same_file::Handle;

use crate::command_stdin::CommandStdin;
use crate::{
    CommandError,
    OutputStream,
};

/// Opened stdin path, file handle, and optional buffered input bytes.
type PreparedInputParts = (Option<PathBuf>, Option<File>, Option<Vec<u8>>);

/// Prepared command-side stdin and runner-side output files.
pub(crate) struct IoFiles {
    /// Bytes written to piped stdin after spawning, if configured.
    pub(crate) stdin_bytes: Option<Vec<u8>>,
    /// Open, validated, and truncated stdout tee file.
    pub(crate) stdout_file: Option<File>,
    /// Open, validated, and truncated stderr tee file.
    pub(crate) stderr_file: Option<File>,
}

impl IoFiles {
    /// Opens, validates, and configures all command I/O files.
    ///
    /// # Parameters
    ///
    /// * `command` - Sanitized command text used in errors.
    /// * `stdin` - Requested stdin configuration.
    /// * `stdout_path` - Optional stdout tee path.
    /// * `stderr_path` - Optional stderr tee path.
    /// * `process_command` - Process command receiving stdin configuration.
    ///
    /// # Returns
    ///
    /// Validated I/O resources ready for process spawning.
    ///
    /// # Errors
    ///
    /// Returns [CommandError] when an input or output cannot be prepared, or
    /// when multiple configured streams identify the same file.
    pub(crate) fn prepare(
        command: &str,
        stdin: CommandStdin,
        stdout_path: Option<&Path>,
        stderr_path: Option<&Path>,
        process_command: &mut ProcessCommand,
    ) -> Result<Self, CommandError> {
        let (stdin_path, stdin_file, stdin_bytes) =
            open_input(command, stdin, process_command)?;
        validate_normalized_paths(
            command,
            stdin_path.as_deref(),
            stdout_path,
            stderr_path,
        )?;

        let stdout_file =
            open_output(command, OutputStream::Stdout, stdout_path)?;
        let stderr_file =
            open_output(command, OutputStream::Stderr, stderr_path)?;

        validate_file_identities(
            command,
            stdin_path.as_deref().zip(stdin_file.as_ref()),
            stdout_path.zip(stdout_file.as_ref()),
            stderr_path.zip(stderr_file.as_ref()),
        )?;
        truncate_output(
            command,
            OutputStream::Stdout,
            stdout_path,
            stdout_file.as_ref(),
        )?;
        truncate_output(
            command,
            OutputStream::Stderr,
            stderr_path,
            stderr_file.as_ref(),
        )?;
        if let Some(file) = stdin_file {
            process_command.stdin(Stdio::from(file));
        }

        Ok(Self {
            stdin_bytes,
            stdout_file,
            stderr_file,
        })
    }
}

/// Opens stdin or configures a non-file stdin mode.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `stdin` - Requested stdin configuration.
/// * `process_command` - Process command receiving non-file configuration.
///
/// # Returns
///
/// Optional input path, opened file, and buffered bytes.
///
/// # Errors
///
/// Returns [CommandError::OpenInputFailed] when a configured file cannot be
/// opened.
fn open_input(
    command: &str,
    stdin: CommandStdin,
    process_command: &mut ProcessCommand,
) -> Result<PreparedInputParts, CommandError> {
    match stdin {
        CommandStdin::Null => {
            process_command.stdin(Stdio::null());
            Ok((None, None, None))
        }
        CommandStdin::Inherit => {
            process_command.stdin(Stdio::inherit());
            Ok((None, None, None))
        }
        CommandStdin::Bytes(bytes) => {
            process_command.stdin(Stdio::piped());
            Ok((None, None, Some(bytes)))
        }
        CommandStdin::File(path) => {
            let file = File::open(&path).map_err(|source| {
                CommandError::OpenInputFailed {
                    command: command.to_owned(),
                    path: path.clone(),
                    source,
                }
            })?;
            Ok((Some(path), Some(file), None))
        }
    }
}

/// Opens one output without truncating it.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `stream` - Output stream receiving the file.
/// * `path` - Optional tee path.
///
/// # Returns
///
/// An open output file when a path is configured.
///
/// # Errors
///
/// Returns [CommandError::OpenOutputFailed] when the file cannot be opened.
fn open_output(
    command: &str,
    stream: OutputStream,
    path: Option<&Path>,
) -> Result<Option<File>, CommandError> {
    path.map(|path| {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|source| CommandError::OpenOutputFailed {
                command: command.to_owned(),
                stream,
                path: path.to_path_buf(),
                source,
            })
    })
    .transpose()
}

/// Rejects paths that normalize to the same filesystem location.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `stdin_path` - Optional configured stdin path.
/// * `stdout_path` - Optional configured stdout tee path.
/// * `stderr_path` - Optional configured stderr tee path.
///
/// # Returns
///
/// `Ok(())` when no normalized paths conflict.
///
/// # Errors
///
/// Returns a conflict error for equal normalized paths, or
/// [CommandError::InspectIoFileFailed] when normalization fails.
fn validate_normalized_paths(
    command: &str,
    stdin_path: Option<&Path>,
    stdout_path: Option<&Path>,
    stderr_path: Option<&Path>,
) -> Result<(), CommandError> {
    let stdin = normalized_path(command, stdin_path)?;
    let stdout = normalized_path(command, stdout_path)?;
    let stderr = normalized_path(command, stderr_path)?;

    if let (Some(input), Some(output)) = (&stdin, &stdout)
        && input == output
    {
        return Err(input_output_conflict(
            command,
            stdin_path.expect("normalized stdin has an original path"),
            OutputStream::Stdout,
            stdout_path.expect("normalized stdout has an original path"),
        ));
    }
    if let (Some(input), Some(output)) = (&stdin, &stderr)
        && input == output
    {
        return Err(input_output_conflict(
            command,
            stdin_path.expect("normalized stdin has an original path"),
            OutputStream::Stderr,
            stderr_path.expect("normalized stderr has an original path"),
        ));
    }
    if let (Some(stdout), Some(stderr)) = (&stdout, &stderr)
        && stdout == stderr
    {
        return Err(output_files_conflict(
            command,
            stdout_path.expect("normalized stdout has an original path"),
            stderr_path.expect("normalized stderr has an original path"),
        ));
    }
    Ok(())
}

/// Resolves one optional path for non-destructive comparison.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `path` - Optional configured I/O path.
///
/// # Returns
///
/// A canonical or lexically normalized absolute path when configured.
///
/// # Errors
///
/// Returns [CommandError::InspectIoFileFailed] when the current directory or
/// an existing path cannot be resolved.
fn normalized_path(
    command: &str,
    path: Option<&Path>,
) -> Result<Option<PathBuf>, CommandError> {
    path.map(|path| {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|source| inspect_error(command, path, source))?
                .join(path)
        };
        match fs::canonicalize(&absolute) {
            Ok(path) => Ok(path),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                Ok(normalize_lexically(&absolute))
            }
            Err(source) => Err(inspect_error(command, path, source)),
        }
    })
    .transpose()
}

/// Normalizes current- and parent-directory path components lexically.
///
/// # Parameters
///
/// * `path` - Absolute or relative path to normalize.
///
/// # Returns
///
/// Lexically normalized path.
#[must_use]
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Rejects open files that identify the same filesystem object.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `stdin` - Optional stdin path and file.
/// * `stdout` - Optional stdout path and file.
/// * `stderr` - Optional stderr path and file.
///
/// # Returns
///
/// `Ok(())` when no open files identify the same filesystem object.
///
/// # Errors
///
/// Returns a conflict error for identical handles, or
/// [CommandError::InspectIoFileFailed] when a handle cannot be inspected.
fn validate_file_identities(
    command: &str,
    stdin: Option<(&Path, &File)>,
    stdout: Option<(&Path, &File)>,
    stderr: Option<(&Path, &File)>,
) -> Result<(), CommandError> {
    let stdin_handle = file_handle(command, stdin)?;
    let stdout_handle = file_handle(command, stdout)?;
    let stderr_handle = file_handle(command, stderr)?;

    if let (Some(input), Some(output)) = (&stdin_handle, &stdout_handle)
        && input == output
    {
        let (input_path, _) = stdin.expect("stdin handle has an original path");
        let (output_path, _) =
            stdout.expect("stdout handle has an original path");
        return Err(input_output_conflict(
            command,
            input_path,
            OutputStream::Stdout,
            output_path,
        ));
    }
    if let (Some(input), Some(output)) = (&stdin_handle, &stderr_handle)
        && input == output
    {
        let (input_path, _) = stdin.expect("stdin handle has an original path");
        let (output_path, _) =
            stderr.expect("stderr handle has an original path");
        return Err(input_output_conflict(
            command,
            input_path,
            OutputStream::Stderr,
            output_path,
        ));
    }
    if let (Some(stdout_handle), Some(stderr_handle)) =
        (&stdout_handle, &stderr_handle)
        && stdout_handle == stderr_handle
    {
        let (stdout_path, _) =
            stdout.expect("stdout handle has an original path");
        let (stderr_path, _) =
            stderr.expect("stderr handle has an original path");
        return Err(output_files_conflict(command, stdout_path, stderr_path));
    }
    Ok(())
}

/// Builds a comparable identity handle for one optional open file.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `file` - Optional configured path and open file.
///
/// # Returns
///
/// Optional comparable identity handle.
///
/// # Errors
///
/// Returns [CommandError::InspectIoFileFailed] when cloning or inspecting
/// the file fails.
fn file_handle(
    command: &str,
    file: Option<(&Path, &File)>,
) -> Result<Option<Handle>, CommandError> {
    file.map(|(path, file)| {
        let clone = file
            .try_clone()
            .map_err(|source| inspect_error(command, path, source))?;
        Handle::from_file(clone)
            .map_err(|source| inspect_error(command, path, source))
    })
    .transpose()
}

/// Truncates one validated regular output file.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `stream` - Output stream receiving the file.
/// * `path` - Optional configured path.
/// * `file` - Optional open output file.
///
/// # Returns
///
/// `Ok(())` after a configured regular file has been truncated. Special files
/// such as devices are left intact and receive output through normal writes.
///
/// # Errors
///
/// Returns [CommandError::InspectIoFileFailed] when file metadata cannot be
/// read, or [CommandError::OpenOutputFailed] when truncation fails.
fn truncate_output(
    command: &str,
    stream: OutputStream,
    path: Option<&Path>,
    file: Option<&File>,
) -> Result<(), CommandError> {
    if let Some(file) = file {
        let path = path.expect("an open output file has an original path");
        let file_type = file
            .metadata()
            .map_err(|source| inspect_error(command, path, source))?
            .file_type();
        if file_type.is_file() {
            file.set_len(0).map_err(|source| {
                CommandError::OpenOutputFailed {
                    command: command.to_owned(),
                    stream,
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        }
    }
    Ok(())
}

/// Builds an input/output conflict error.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `input_path` - Configured stdin path.
/// * `output_stream` - Output stream whose tee path conflicts with stdin.
/// * `output_path` - Conflicting output tee path.
///
/// # Returns
///
/// Structured conflict error retaining both configured paths.
#[must_use]
#[inline]
fn input_output_conflict(
    command: &str,
    input_path: &Path,
    output_stream: OutputStream,
    output_path: &Path,
) -> CommandError {
    CommandError::InputOutputConflict {
        command: command.to_owned(),
        input_path: input_path.to_path_buf(),
        output_stream,
        output_path: output_path.to_path_buf(),
    }
}

/// Builds an output/output conflict error.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `stdout_path` - Configured stdout tee path.
/// * `stderr_path` - Conflicting stderr tee path.
///
/// # Returns
///
/// Structured conflict error retaining both configured paths.
#[must_use]
#[inline]
fn output_files_conflict(
    command: &str,
    stdout_path: &Path,
    stderr_path: &Path,
) -> CommandError {
    CommandError::OutputFilesConflict {
        command: command.to_owned(),
        stdout_path: stdout_path.to_path_buf(),
        stderr_path: stderr_path.to_path_buf(),
    }
}

/// Builds an I/O-file inspection error.
///
/// # Parameters
///
/// * `command` - Sanitized command text used in errors.
/// * `path` - Configured path that could not be inspected.
/// * `source` - Underlying filesystem error.
///
/// # Returns
///
/// Structured inspection error retaining the configured path.
#[must_use]
#[inline]
fn inspect_error(
    command: &str,
    path: &Path,
    source: io::Error,
) -> CommandError {
    CommandError::InspectIoFileFailed {
        command: command.to_owned(),
        path: path.to_path_buf(),
        source,
    }
}
