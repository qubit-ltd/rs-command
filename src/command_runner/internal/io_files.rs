// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! Prepares command I/O files without truncating conflicting paths.

use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::process::Stdio;
#[cfg(coverage)]
use std::sync::atomic::AtomicBool;
#[cfg(coverage)]
use std::sync::atomic::Ordering;

use same_file::Handle;

use crate::CommandError;
use crate::OutputStream;
use crate::command_stdin::CommandStdin;

#[cfg(coverage)]
static COVERAGE_FAIL_TRUNCATE: AtomicBool = AtomicBool::new(false);

/// Enables or disables deterministic truncation failure injection.
#[cfg(coverage)]
pub(in crate::command_runner) fn __coverage_fail_truncate(enabled: bool) {
    COVERAGE_FAIL_TRUNCATE.store(enabled, Ordering::Relaxed);
}

/// Opened stdin path, file handle, and optional buffered input bytes.
type PreparedInputParts = (Option<PathBuf>, Option<File>, Option<Vec<u8>>);

/// Opens a stdin candidate with platform-specific special-file protection.
///
/// Unix callers receive a descriptor opened with `O_NONBLOCK`, so opening a
/// FIFO does not wait for a peer. Other platforms use their native file open
/// operation and rely on the subsequent handle-authoritative type check.
///
/// # Parameters
///
/// * `path` - Candidate path to open for reading.
///
/// # Returns
///
/// An open file handle whose metadata must be validated before use.
///
/// # Errors
///
/// Returns the operating-system error reported while opening `path`.
#[cfg(unix)]
pub(in crate::command_runner) fn open_input_candidate(
    path: &Path,
) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NONBLOCK);
    options.open(path)
}

/// Opens a stdin candidate on platforms without Unix open flags.
#[cfg(not(unix))]
pub(in crate::command_runner) fn open_input_candidate(
    path: &Path,
) -> io::Result<File> {
    File::open(path)
}

/// Opens an output candidate with platform-specific special-file protection.
///
/// # Parameters
///
/// * `path` - Candidate path to open for writing.
///
/// # Returns
///
/// An open file handle whose metadata must be validated before use.
///
/// # Errors
///
/// Returns the operating-system error reported while opening `path`.
#[cfg(unix)]
pub(in crate::command_runner) fn open_output_candidate(
    path: &Path,
) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .create(true)
        .write(true)
        .truncate(false)
        .custom_flags(libc::O_NONBLOCK);
    options.open(path)
}

/// Opens an output candidate on platforms without Unix open flags.
#[cfg(not(unix))]
pub(in crate::command_runner) fn open_output_candidate(
    path: &Path,
) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
}

/// Clears the temporary nonblocking flag from a validated Unix file handle.
///
/// # Parameters
///
/// * `file` - Live descriptor opened with `O_NONBLOCK`.
///
/// # Returns
///
/// `Ok(())` after the descriptor is blocking, or immediately when it was
/// already blocking.
///
/// # Errors
///
/// Returns the native error from either `fcntl` operation.
#[cfg(unix)]
fn clear_nonblocking(file: &File) -> io::Result<()> {
    // SAFETY: `file` owns a live descriptor throughout both non-retaining
    // `fcntl` calls.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if flags & libc::O_NONBLOCK == 0 {
        return Ok(());
    }
    // SAFETY: `F_SETFL` accepts the status flags returned by `F_GETFL` with
    // only `O_NONBLOCK` cleared, and the descriptor remains live.
    let result = unsafe {
        libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK)
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Leaves file blocking state unchanged on non-Unix platforms.
#[cfg(not(unix))]
fn clear_nonblocking(_file: &File) -> io::Result<()> {
    Ok(())
}

/// Prepared command-side stdin and runner-side output files.
pub(in crate::command_runner) struct IoFiles {
    /// Bytes written to piped stdin after spawning, if configured.
    pub(in crate::command_runner) stdin_bytes: Option<Vec<u8>>,
    /// Open, validated, and truncated stdout tee file.
    pub(in crate::command_runner) stdout_file: Option<File>,
    /// Open, validated, and truncated stderr tee file.
    pub(in crate::command_runner) stderr_file: Option<File>,
}

impl IoFiles {
    /// Opens, validates, and configures all command I/O files.
    ///
    /// # Parameters
    ///
    /// * `command` - Redacted command text used in errors.
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
    pub(in crate::command_runner) fn prepare(
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
/// * `command` - Redacted command text used in errors.
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
            ensure_regular_input(command, &path)?;
            let file = open_input_candidate(&path).map_err(|source| {
                CommandError::OpenInputFailed {
                    command: command.to_owned(),
                    path: path.clone(),
                    source,
                }
            })?;
            ensure_regular_input_handle(command, &path, &file)?;
            clear_nonblocking(&file).map_err(|source| {
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
/// * `command` - Redacted command text used in errors.
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
        ensure_regular_output(command, stream, path)?;
        let file = open_output_candidate(path).map_err(|source| {
            CommandError::OpenOutputFailed {
                command: command.to_owned(),
                stream,
                path: path.to_path_buf(),
                source,
            }
        })?;
        ensure_regular_output_handle(command, stream, path, &file)?;
        clear_nonblocking(&file).map_err(|source| {
            CommandError::OpenOutputFailed {
                command: command.to_owned(),
                stream,
                path: path.to_path_buf(),
                source,
            }
        })?;
        Ok(file)
    })
    .transpose()
}

/// Validates the metadata of the opened stdin handle.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `path` - Configured stdin path.
/// * `file` - Open handle whose metadata is authoritative.
///
/// # Returns
///
/// `Ok(())` when the handle identifies an ordinary file.
///
/// # Errors
///
/// Returns [`CommandError::OpenInputFailed`] when handle metadata cannot be
/// read, or [`CommandError::NonRegularInputFile`] for another file type.
pub(in crate::command_runner) fn ensure_regular_input_handle(
    command: &str,
    path: &Path,
    file: &File,
) -> Result<(), CommandError> {
    let metadata =
        file.metadata()
            .map_err(|source| CommandError::OpenInputFailed {
                command: command.to_owned(),
                path: path.to_path_buf(),
                source,
            })?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(CommandError::NonRegularInputFile {
            command: command.to_owned(),
            path: path.to_path_buf(),
        })
    }
}

/// Validates the metadata of an opened output handle.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `stream` - Output stream receiving the file.
/// * `path` - Configured output path.
/// * `file` - Open handle whose metadata is authoritative.
///
/// # Returns
///
/// `Ok(())` when the handle identifies an ordinary file.
///
/// # Errors
///
/// Returns [`CommandError::OpenOutputFailed`] when handle metadata cannot be
/// read, or [`CommandError::NonRegularOutputFile`] for another file type.
pub(in crate::command_runner) fn ensure_regular_output_handle(
    command: &str,
    stream: OutputStream,
    path: &Path,
    file: &File,
) -> Result<(), CommandError> {
    let metadata =
        file.metadata()
            .map_err(|source| CommandError::OpenOutputFailed {
                command: command.to_owned(),
                stream,
                path: path.to_path_buf(),
                source,
            })?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(CommandError::NonRegularOutputFile {
            command: command.to_owned(),
            stream,
            path: path.to_path_buf(),
        })
    }
}

/// Ensures that a configured stdin path identifies an ordinary file.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `path` - Configured stdin path.
///
/// # Returns
///
/// `Ok(())` when `path` exists and is an ordinary file.
///
/// # Errors
///
/// Returns [`CommandError::OpenInputFailed`] when metadata cannot be read, or
/// [`CommandError::NonRegularInputFile`] when the path identifies another file
/// type.
fn ensure_regular_input(
    command: &str,
    path: &Path,
) -> Result<(), CommandError> {
    let metadata =
        fs::metadata(path).map_err(|source| CommandError::OpenInputFailed {
            command: command.to_owned(),
            path: path.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(CommandError::NonRegularInputFile {
            command: command.to_owned(),
            path: path.to_path_buf(),
        })
    }
}

/// Ensures that a configured output path is or will become an ordinary file.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `stream` - Output stream receiving the tee.
/// * `path` - Configured output path.
///
/// # Returns
///
/// `Ok(())` when an existing path is an ordinary file or the path does not yet
/// exist and may be created by [`open_output`].
///
/// # Errors
///
/// Returns [`CommandError::NonRegularOutputFile`] when an existing path is not
/// an ordinary file, or [`CommandError::InspectIoFileFailed`] when metadata
/// inspection fails for a reason other than absence.
fn ensure_regular_output(
    command: &str,
    stream: OutputStream,
    path: &Path,
) -> Result<(), CommandError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(CommandError::NonRegularOutputFile {
            command: command.to_owned(),
            stream,
            path: path.to_path_buf(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            match fs::symlink_metadata(path) {
                Ok(_) => Err(CommandError::NonRegularOutputFile {
                    command: command.to_owned(),
                    stream,
                    path: path.to_path_buf(),
                }),
                Err(link_source)
                    if link_source.kind() == io::ErrorKind::NotFound =>
                {
                    Ok(())
                }
                Err(link_source) => {
                    Err(inspect_error(command, path, link_source))
                }
            }
        }
        Err(source) => Err(inspect_error(command, path, source)),
    }
}

/// Rejects paths that normalize to the same filesystem location.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
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
///
/// # Panics
///
/// Panics when normalized paths exist without their corresponding original
/// input paths, which indicates an internal invariant violation.
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
/// * `command` - Redacted command text used in errors.
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
pub(in crate::command_runner) fn normalize_lexically(path: &Path) -> PathBuf {
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
/// * `command` - Redacted command text used in errors.
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
///
/// # Panics
///
/// Panics when an inspected handle exists without its original configured
/// path, which indicates an internal invariant violation.
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
/// * `command` - Redacted command text used in errors.
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
/// * `command` - Redacted command text used in errors.
/// * `stream` - Output stream receiving the file.
/// * `path` - Optional configured path.
/// * `file` - Optional open output file.
///
/// # Returns
///
/// `Ok(())` after a configured regular file has been truncated.
///
/// # Errors
///
/// Returns [CommandError::InspectIoFileFailed] when file metadata cannot be
/// read, or [CommandError::OpenOutputFailed] when truncation fails.
///
/// # Panics
///
/// Panics when an open tee file has no corresponding configured path, which
/// indicates an internal invariant violation.
pub(in crate::command_runner) fn truncate_output(
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
        if !file_type.is_file() {
            return Err(CommandError::NonRegularOutputFile {
                command: command.to_owned(),
                stream,
                path: path.to_path_buf(),
            });
        }
        #[cfg(coverage)]
        if COVERAGE_FAIL_TRUNCATE.load(Ordering::Relaxed) {
            return Err(CommandError::OpenOutputFailed {
                command: command.to_owned(),
                stream,
                path: path.to_path_buf(),
                source: io::Error::other(
                    "coverage-injected output truncation failure",
                ),
            });
        }
        file.set_len(0)
            .map_err(|source| CommandError::OpenOutputFailed {
                command: command.to_owned(),
                stream,
                path: path.to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

/// Builds an input/output conflict error.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
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
/// * `command` - Redacted command text used in errors.
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
/// * `command` - Redacted command text used in errors.
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
