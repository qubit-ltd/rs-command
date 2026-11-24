// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Prepares command I/O files without truncating conflicting paths.

use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::process::Stdio;

use same_file::Handle;

use super::file_tee::FileTee;
use crate::CommandError;
use crate::CommandErrorReason;
use crate::OutputStream;
use crate::command_stdin::CommandStdin;
use crate::execution::contracts::PreparedFiles;

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
pub(in crate::command_runner) fn open_input_candidate(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NONBLOCK);
    options.open(path)
}

/// Opens a stdin candidate on platforms without Unix open flags.
///
/// # Parameters
///
/// * `path` - Candidate path to open for reading.
///
/// # Returns
///
/// Returns an open file handle; callers must validate its type before use.
/// Opening the candidate does not establish that it is a regular file or
/// provide a bounded open operation.
///
/// # Errors
///
/// Returns the operating-system error reported while opening `path`.
#[cfg(not(unix))]
pub(in crate::command_runner) fn open_input_candidate(path: &Path) -> io::Result<File> {
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
pub(in crate::command_runner) fn open_output_candidate(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .create(true)
        .write(true)
        .truncate(false)
        .custom_flags(libc::O_NONBLOCK);
    options.open(path)
}

/// Opens an output candidate on platforms without Unix open flags.
///
/// # Parameters
///
/// * `path` - Candidate path to open for writing.
///
/// # Returns
///
/// Returns an open file handle; callers must validate its type before use.
/// The open may create a new file but does not truncate an existing file.
/// Opening the candidate does not establish that it is a regular file or
/// provide a bounded open operation.
///
/// # Errors
///
/// Returns the operating-system error reported while opening `path`.
#[cfg(not(unix))]
pub(in crate::command_runner) fn open_output_candidate(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).write(true).truncate(false).open(path)
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
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Leaves file blocking state unchanged on non-Unix platforms.
///
/// # Parameters
///
/// * `_file` - File whose blocking state is left unchanged.
#[cfg(not(unix))]
fn clear_nonblocking(_file: &File) -> io::Result<()> {
    Ok(())
}

/// Prepared command-side stdin and runner-side output files.
pub(in crate::command_runner) struct IoFiles {
    /// Bytes written to piped stdin after spawning, if configured.
    pub(in crate::command_runner) stdin_bytes: Option<Vec<u8>>,
    /// Open and validated stdin file retained until commit.
    pub(in crate::command_runner) stdin_path: Option<PathBuf>,
    /// Open and validated stdin file retained until commit.
    pub(in crate::command_runner) stdin_file: Option<File>,
    /// Open, validated, and truncated stdout tee file after commit.
    pub(in crate::command_runner) stdout_tee: Option<FileTee>,
    /// Open, validated, and truncated stderr tee file after commit.
    pub(in crate::command_runner) stderr_tee: Option<FileTee>,
}

impl IoFiles {
    /// Performs non-destructive command I/O validation.
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
    /// Input resources and output paths validated without creating or
    /// truncating output files.
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
        let (stdin_path, stdin_file, stdin_bytes) = open_input(command, stdin, process_command)?;
        validate_normalized_paths(command, stdin_path.as_deref(), stdout_path, stderr_path)?;

        if let Some(path) = stdout_path {
            ensure_regular_output(command, OutputStream::Stdout, path)?;
        }
        if let Some(path) = stderr_path {
            ensure_regular_output(command, OutputStream::Stderr, path)?;
        }

        Ok(Self {
            stdin_bytes,
            stdin_path,
            stdin_file,
            stdout_tee: None,
            stderr_tee: None,
        })
    }

    /// Commits output file creation, truncation, and stdin attachment.
    ///
    /// # Parameters
    ///
    /// * `command` - Redacted command text used in errors.
    /// * `stdout_path` - Optional stdout tee path; `None` leaves stdout without
    ///   a tee.
    /// * `stderr_path` - Optional stderr tee path; `None` leaves stderr without
    ///   a tee.
    /// * `process_command` - Process command to receive the prepared stdin
    ///   file, if any; the file handle is transferred on success.
    ///
    /// Output files are opened in stdout-then-stderr order, then their opened
    /// handles are checked for file identity conflicts with each other and
    /// stdin. Only after validation are the files truncated in stdout-then-
    /// stderr order. This method does not spawn the process. If opening or
    /// truncating a later file fails, earlier files may already have been
    /// created or truncated; those filesystem changes are not rolled back.
    ///
    /// # Returns
    ///
    /// `Ok(())` when configured output tees are open, validated, and truncated
    /// and retained by this `IoFiles`, and any prepared stdin file has been
    /// attached to `process_command`.
    /// With no configured output paths, no output files are opened or
    /// truncated.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if an output cannot be opened or inspected, is
    /// not a regular file, conflicts with stdin or the other output file, or
    /// cannot be truncated. A failure can occur after earlier file creation
    /// or truncation side effects.
    #[cfg(test)]
    pub(in crate::command_runner) fn commit(
        &mut self,
        command: &str,
        stdout_path: Option<&Path>,
        stderr_path: Option<&Path>,
        process_command: &mut ProcessCommand,
    ) -> Result<(), CommandError> {
        self.commit_with_managed_files(command, stdout_path, stderr_path, process_command, None)
    }

    /// Opens configured tee files, checks their identities against managed
    /// files, and attaches stdin only after validation succeeds.
    ///
    /// # Parameters
    ///
    /// * `command` - Redacted command text used to construct errors.
    /// * `stdout_path` - Optional path receiving child stdout.
    /// * `stderr_path` - Optional path receiving child stderr.
    /// * `process_command` - Process command that receives the validated stdin.
    /// * `managed_files` - Registered resource files to compare with command
    ///   I/O files.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if a tee cannot be opened, a file identity
    /// conflicts, or output truncation fails. Earlier file creation or
    /// truncation side effects may remain when a later step fails.
    pub(in crate::command_runner) fn commit_with_managed_files(
        &mut self,
        command: &str,
        stdout_path: Option<&Path>,
        stderr_path: Option<&Path>,
        process_command: &mut ProcessCommand,
        managed_files: Option<&PreparedFiles>,
    ) -> Result<(), CommandError> {
        let stdout_tee = open_output(command, OutputStream::Stdout, stdout_path)?;
        let stderr_tee = open_output(command, OutputStream::Stderr, stderr_path)?;
        validate_file_identities(
            command,
            self.stdin_path.as_deref().zip(self.stdin_file.as_ref()),
            stdout_tee.as_ref().map(|tee| (tee.path(), tee.file())),
            stderr_tee.as_ref().map(|tee| (tee.path(), tee.file())),
        )?;
        validate_managed_file_identities(
            command,
            self.stdin_file.as_ref(),
            stdout_tee.as_ref().map(FileTee::file),
            stderr_tee.as_ref().map(FileTee::file),
            managed_files,
        )?;
        truncate_outputs_with(
            command,
            stdout_tee.as_ref().map(FileTee::path),
            stdout_tee.as_ref().map(FileTee::file),
            stderr_tee.as_ref().map(FileTee::path),
            stderr_tee.as_ref().map(FileTee::file),
            truncate_output,
        )?;
        if let Some(file) = self.stdin_file.take() {
            process_command.stdin(Stdio::from(file));
        }
        self.stdout_tee = stdout_tee;
        self.stderr_tee = stderr_tee;
        Ok(())
    }
}

/// Compares command file handles with the handles retained by managed
/// resources.
///
/// # Parameters
///
/// * `command` - Redacted command text used to construct errors.
/// * `command_stdin` - Optional opened command stdin handle.
/// * `stdout_tee` - Optional opened stdout tee handle.
/// * `stderr_tee` - Optional opened stderr tee handle.
/// * `managed` - Registered handles retained by the managed resources.
///
/// # Errors
///
/// Returns [`CommandError`] when handles cannot be cloned or identify the same
/// file across configured streams.
fn validate_managed_file_identities(
    command: &str,
    command_stdin: Option<&File>,
    stdout_tee: Option<&File>,
    stderr_tee: Option<&File>,
    managed: Option<&PreparedFiles>,
) -> Result<(), CommandError> {
    let Some(managed) = managed else {
        return Ok(());
    };
    let mut opened = Vec::new();
    if let Some(file) = command_stdin {
        opened.push((
            "stdin",
            file.try_clone()
                .map_err(|source| managed_identity_error(command, "stdin", source))?,
        ));
    }
    if let Some(resource) = managed.stdin.as_ref() {
        let file = resource
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_clone()
            .map_err(|source| managed_identity_error(command, "stdin", source))?;
        opened.push(("stdin", file));
    }
    for (direction, sink) in [("stdout", managed.stdout.as_ref()), ("stderr", managed.stderr.as_ref())] {
        if let Some(sink) = sink {
            let file = sink
                .file
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .try_clone()
                .map_err(|source| managed_identity_error(command, direction, source))?;
            opened.push((direction, file));
        }
    }
    if let Some(file) = stdout_tee {
        opened.push((
            "stdout",
            file.try_clone()
                .map_err(|source| managed_identity_error(command, "stdout", source))?,
        ));
    }
    if let Some(file) = stderr_tee {
        opened.push((
            "stderr",
            file.try_clone()
                .map_err(|source| managed_identity_error(command, "stderr", source))?,
        ));
    }
    let mut identities = Vec::with_capacity(opened.len());
    for (direction, file) in &opened {
        let identity = Handle::from_file(
            file.try_clone()
                .map_err(|source| managed_identity_error(command, direction, source))?,
        )
        .map_err(|source| managed_identity_error(command, direction, source))?;
        if identities.iter().any(|(_, existing)| existing == &identity) {
            return Err(managed_identity_error(
                command,
                direction,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "configured file streams identify the same file",
                ),
            ));
        }
        identities.push((direction, identity));
    }
    Ok(())
}

/// Associates a managed file identity error with its stream and validation
/// stage while retaining the original I/O error as its source.
///
/// # Parameters
///
/// * `command` - Redacted command text used in the resulting error.
/// * `direction` - Standard stream whose identity check failed.
/// * `source` - Original file identity or handle I/O failure.
fn managed_identity_error(command: &str, direction: &str, source: io::Error) -> CommandError {
    let direction = match direction {
        "stdin" => crate::FilePreparationDirection::Stdin,
        "stdout" => crate::FilePreparationDirection::Stdout,
        "stderr" => crate::FilePreparationDirection::Stderr,
        _ => unreachable!("all managed file directions are known"),
    };
    CommandError::from_reason(
        command,
        CommandErrorReason::FilePreparationFailed {
            stage: crate::FilePreparationStage::Validation,
            direction: Some(direction),
            source,
        },
        None,
    )
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
/// Returns [CommandError] when a configured file cannot be
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
            let file = open_input_candidate(&path).map_err(|source| open_input_error(command, &path, source))?;
            ensure_regular_input_handle(command, &path, &file)?;
            clear_nonblocking(&file).map_err(|source| open_input_error(command, &path, source))?;
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
/// Returns [CommandError] when the file cannot be opened.
fn open_output(command: &str, stream: OutputStream, path: Option<&Path>) -> Result<Option<FileTee>, CommandError> {
    path.map(|path| {
        ensure_regular_output(command, stream, path)?;
        let file = open_output_candidate(path).map_err(|source| open_output_error(command, stream, path, source))?;
        ensure_regular_output_handle(command, stream, path, &file)?;
        clear_nonblocking(&file).map_err(|source| open_output_error(command, stream, path, source))?;
        Ok(FileTee::new(file, path.to_path_buf()))
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
/// Returns [`CommandError`] when handle metadata cannot be
/// read, or [`CommandError`] for another file type.
pub(in crate::command_runner) fn ensure_regular_input_handle(
    command: &str,
    path: &Path,
    file: &File,
) -> Result<(), CommandError> {
    let metadata = file
        .metadata()
        .map_err(|source| open_input_error(command, path, source))?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(non_regular_input_error(command, path))
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
/// Returns [`CommandError`] when handle metadata cannot be
/// read, or [`CommandError`] for another file type.
pub(in crate::command_runner) fn ensure_regular_output_handle(
    command: &str,
    stream: OutputStream,
    path: &Path,
    file: &File,
) -> Result<(), CommandError> {
    let metadata = file
        .metadata()
        .map_err(|source| open_output_error(command, stream, path, source))?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(non_regular_output_error(command, stream, path))
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
/// Returns [`CommandError`] when metadata cannot be read, or
/// [`CommandError`] when the path identifies another file
/// type.
fn ensure_regular_input(command: &str, path: &Path) -> Result<(), CommandError> {
    let metadata = fs::metadata(path).map_err(|source| open_input_error(command, path, source))?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(non_regular_input_error(command, path))
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
/// Returns [`CommandError`] when an existing path is not
/// an ordinary file, or [`CommandError`] when metadata
/// inspection fails for a reason other than absence.
fn ensure_regular_output(command: &str, stream: OutputStream, path: &Path) -> Result<(), CommandError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(non_regular_output_error(command, stream, path)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => match fs::symlink_metadata(path) {
            Ok(_) => Err(non_regular_output_error(command, stream, path)),
            Err(link_source) if link_source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(link_source) => Err(inspect_error(command, path, link_source)),
        },
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
/// [CommandError] when normalization fails.
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

    if let (Some(Some(input)), Some(Some(output))) = (&stdin, &stdout)
        && input == output
    {
        return Err(input_output_conflict(
            command,
            stdin_path.expect("normalized stdin has an original path"),
            OutputStream::Stdout,
            stdout_path.expect("normalized stdout has an original path"),
        ));
    }
    if let (Some(Some(input)), Some(Some(output))) = (&stdin, &stderr)
        && input == output
    {
        return Err(input_output_conflict(
            command,
            stdin_path.expect("normalized stdin has an original path"),
            OutputStream::Stderr,
            stderr_path.expect("normalized stderr has an original path"),
        ));
    }
    if let (Some(Some(stdout)), Some(Some(stderr))) = (&stdout, &stderr)
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
/// A canonical absolute path when configured and resolvable. A configured
/// path whose parent does not yet exist is represented as indeterminate so
/// normal file opening reports the error.
///
/// # Errors
///
/// Returns [CommandError] when the current directory or
/// an existing path cannot be resolved.
fn normalized_path(command: &str, path: Option<&Path>) -> Result<Option<Option<PathBuf>>, CommandError> {
    path.map(|path| {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|source| inspect_error(command, path, source))?
                .join(path)
        };
        match fs::canonicalize(&absolute) {
            Ok(path) => Ok(Some(path)),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let Some(file_name) = absolute.file_name() else {
                    return Ok(None);
                };
                let parent = absolute.parent().expect("absolute file path has a parent");
                match fs::canonicalize(parent) {
                    Ok(parent) => Ok(Some(parent.join(file_name))),
                    Err(parent_error) if parent_error.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(parent_error) => Err(inspect_error(command, path, parent_error)),
                }
            }
            Err(source) => Err(inspect_error(command, path, source)),
        }
    })
    .transpose()
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
/// [CommandError] when a handle cannot be inspected.
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
        let (output_path, _) = stdout.expect("stdout handle has an original path");
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
        let (output_path, _) = stderr.expect("stderr handle has an original path");
        return Err(input_output_conflict(
            command,
            input_path,
            OutputStream::Stderr,
            output_path,
        ));
    }
    if let (Some(stdout_handle), Some(stderr_handle)) = (&stdout_handle, &stderr_handle)
        && stdout_handle == stderr_handle
    {
        let (stdout_path, _) = stdout.expect("stdout handle has an original path");
        let (stderr_path, _) = stderr.expect("stderr handle has an original path");
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
/// Returns [CommandError] when cloning or inspecting
/// the file fails.
fn file_handle(command: &str, file: Option<(&Path, &File)>) -> Result<Option<Handle>, CommandError> {
    file.map(|(path, file)| {
        let clone = file
            .try_clone()
            .map_err(|source| inspect_error(command, path, source))?;
        Handle::from_file(clone).map_err(|source| inspect_error(command, path, source))
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
/// Returns [CommandError] when file metadata cannot be
/// read, or [CommandError] when truncation fails.
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
    truncate_output_with(command, stream, path, file, |file| file.set_len(0))
}

/// Truncates one validated regular output file with the supplied operation.
/// # Parameters
///
/// * `command` - Redacted command text used in any returned diagnostic.
/// * `stream` - Output stream whose state is updated or returned.
/// * `path` - Filesystem path involved in this operation.
/// * `file` - Open file whose descriptor flags are updated.
/// * `truncate` - Operation that truncates the open file.
fn truncate_output_with(
    command: &str,
    stream: OutputStream,
    path: Option<&Path>,
    file: Option<&File>,
    truncate: impl FnOnce(&File) -> io::Result<()>,
) -> Result<(), CommandError> {
    if let Some(file) = file {
        let path = path.expect("an open output file has an original path");
        let file_type = file
            .metadata()
            .map_err(|source| inspect_error(command, path, source))?
            .file_type();
        if !file_type.is_file() {
            return Err(non_regular_output_error(command, stream, path));
        }
        truncate(file).map_err(|source| open_output_error(command, stream, path, source))?;
    }
    Ok(())
}

/// Truncates configured output files in commit order, stdout before stderr.
/// # Parameters
///
/// * `command` - Redacted command text used in diagnostics.
/// * `stdout_path` - Optional stdout destination path.
/// * `stdout_file` - Optional open stdout destination.
/// * `stderr_path` - Optional stderr destination path.
/// * `stderr_file` - Optional open stderr destination.
/// * `truncate` - Operation applied to each configured destination.
fn truncate_outputs_with(
    command: &str,
    stdout_path: Option<&Path>,
    stdout_file: Option<&File>,
    stderr_path: Option<&Path>,
    stderr_file: Option<&File>,
    mut truncate: impl FnMut(&str, OutputStream, Option<&Path>, Option<&File>) -> Result<(), CommandError>,
) -> Result<(), CommandError> {
    truncate(command, OutputStream::Stdout, stdout_path, stdout_file)?;
    truncate(command, OutputStream::Stderr, stderr_path, stderr_file)
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
#[inline]
fn input_output_conflict(
    command: &str,
    input_path: &Path,
    output_stream: OutputStream,
    output_path: &Path,
) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::InputOutputConflict {
            input_path: input_path.to_path_buf(),
            output_stream,
            output_path: output_path.to_path_buf(),
        },
        None,
    )
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
#[inline]
fn output_files_conflict(command: &str, stdout_path: &Path, stderr_path: &Path) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::OutputFilesConflict {
            stdout_path: stdout_path.to_path_buf(),
            stderr_path: stderr_path.to_path_buf(),
        },
        None,
    )
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
#[inline]
fn inspect_error(command: &str, path: &Path, source: io::Error) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::InspectIoFileFailed {
            path: path.to_path_buf(),
            source,
        },
        None,
    )
}

/// Builds an input-opening failure.
/// # Parameters
///
/// * `command` - Redacted command text used in diagnostics.
/// * `path` - Filesystem path involved in this operation.
/// * `source` - Underlying I/O error to inspect or preserve.
fn open_input_error(command: &str, path: &Path, source: io::Error) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::OpenInputFailed {
            path: path.to_path_buf(),
            source,
        },
        None,
    )
}

/// Builds an output-opening failure.
/// # Parameters
///
/// * `command` - Redacted command text used in diagnostics.
/// * `stream` - Stream attributed to this reader.
/// * `path` - Filesystem path involved in this operation.
/// * `source` - Underlying I/O error to inspect or preserve.
fn open_output_error(command: &str, stream: OutputStream, path: &Path, source: io::Error) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::OpenOutputFailed {
            stream,
            path: path.to_path_buf(),
            source,
        },
        None,
    )
}

/// Builds a non-regular input failure.
/// # Parameters
///
/// * `command` - Redacted command text used in diagnostics.
/// * `path` - Filesystem path involved in this operation.
fn non_regular_input_error(command: &str, path: &Path) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::NonRegularInputFile {
            path: path.to_path_buf(),
        },
        None,
    )
}

/// Builds a non-regular output failure.
/// # Parameters
///
/// * `command` - Redacted command text used in diagnostics.
/// * `stream` - Stream attributed to this reader.
/// * `path` - Filesystem path involved in this operation.
fn non_regular_output_error(command: &str, stream: OutputStream, path: &Path) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::NonRegularOutputFile {
            stream,
            path: path.to_path_buf(),
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io;
    use std::process::Command as ProcessCommand;

    use super::IoFiles;
    #[cfg(unix)]
    use super::ensure_regular_input_handle;
    #[cfg(unix)]
    use super::ensure_regular_output_handle;
    use super::truncate_output_with;
    use super::truncate_outputs_with;
    use crate::CommandErrorKind;
    use crate::OutputStream;
    use crate::command_stdin::CommandStdin;

    #[test]
    fn test_prepare_is_non_destructive_and_commit_truncates_and_attaches_files() {
        let stdin = tempfile::NamedTempFile::new().expect("stdin fixture should be created");
        std::fs::write(stdin.path(), b"input").expect("stdin fixture should be populated");
        let stdout = tempfile::NamedTempFile::new().expect("stdout fixture should be created");
        let stderr = tempfile::NamedTempFile::new().expect("stderr fixture should be created");
        std::fs::write(stdout.path(), b"old stdout").expect("stdout should be populated");
        std::fs::write(stderr.path(), b"old stderr").expect("stderr should be populated");
        let mut process = ProcessCommand::new("unused-test-program");

        let mut files = IoFiles::prepare(
            "command",
            CommandStdin::File(stdin.path().to_path_buf()),
            Some(stdout.path()),
            Some(stderr.path()),
            &mut process,
        )
        .expect("regular input and output files should prepare");

        assert_eq!(
            std::fs::read(stdout.path()).expect("stdout should remain readable before commit"),
            b"old stdout",
        );
        assert_eq!(
            std::fs::read(stderr.path()).expect("stderr should remain readable before commit"),
            b"old stderr",
        );
        assert!(files.stdin_file.is_some());
        files
            .commit("command", Some(stdout.path()), Some(stderr.path()), &mut process)
            .expect("commit should attach stdin and prepare both output tees");

        assert_eq!(
            std::fs::read(stdout.path()).expect("stdout should remain readable after commit"),
            b"",
        );
        assert_eq!(
            std::fs::read(stderr.path()).expect("stderr should remain readable after commit"),
            b"",
        );
        assert!(files.stdin_file.is_none());
        assert!(files.stdout_tee.is_some());
        assert!(files.stderr_tee.is_some());
    }

    #[test]
    fn test_prepare_bytes_stdin_configures_pipe_without_output_files() {
        let mut process = ProcessCommand::new("unused-test-program");
        let files = IoFiles::prepare(
            "command",
            CommandStdin::Bytes(b"buffered input".to_vec()),
            None,
            None,
            &mut process,
        )
        .expect("byte stdin should prepare");

        assert_eq!(files.stdin_bytes.as_deref(), Some(b"buffered input".as_slice()));
        assert!(files.stdin_file.is_none());
        assert!(files.stdout_tee.is_none());
        assert!(files.stderr_tee.is_none());
    }

    #[test]
    fn test_prepare_maps_missing_stdin_to_open_input_failure() {
        let directory = tempfile::tempdir().expect("directory fixture should be created");
        let input = directory.path().join("missing-input");
        let mut process = ProcessCommand::new("unused-test-program");

        let error = IoFiles::prepare("command", CommandStdin::File(input.clone()), None, None, &mut process)
            .err()
            .expect("a missing stdin path should be rejected");

        assert_eq!(error.kind(), CommandErrorKind::OpenInputFailed);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::OpenInputFailed { path, source }
                if path == &input && source.kind() == io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn test_prepare_rejects_shared_stdout_and_stderr_path() {
        let directory = tempfile::tempdir().expect("directory fixture should be created");
        let output = directory.path().join("shared-output");
        let mut process = ProcessCommand::new("unused-test-program");

        let error = IoFiles::prepare(
            "command",
            CommandStdin::Null,
            Some(&output),
            Some(&output),
            &mut process,
        )
        .err()
        .expect("stdout and stderr cannot share one destination");

        assert_eq!(error.kind(), CommandErrorKind::OutputFilesConflict);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::OutputFilesConflict { stdout_path, stderr_path }
                if stdout_path == &output && stderr_path == &output
        ));
        assert!(!output.exists(), "preparation must not create output files");
    }

    #[cfg(unix)]
    #[test]
    fn test_commit_rejects_distinct_paths_for_the_same_output_file() {
        let directory = tempfile::tempdir().expect("directory fixture should be created");
        let stdout = directory.path().join("stdout");
        let stderr_alias = directory.path().join("stderr-alias");
        std::fs::write(&stdout, b"preserve until validation").expect("stdout file should be populated");
        std::fs::hard_link(&stdout, &stderr_alias).expect("hard link should share the output file identity");
        let mut process = ProcessCommand::new("unused-test-program");
        let mut files = IoFiles::prepare(
            "command",
            CommandStdin::Null,
            Some(&stdout),
            Some(&stderr_alias),
            &mut process,
        )
        .expect("different normalized output paths should pass preparation");

        let error = files
            .commit("command", Some(&stdout), Some(&stderr_alias), &mut process)
            .expect_err("commit must compare opened file identities");

        assert_eq!(error.kind(), CommandErrorKind::OutputFilesConflict);
        assert_eq!(
            std::fs::read(&stdout).expect("stdout should remain readable"),
            b"preserve until validation"
        );
    }

    #[test]
    fn test_commit_maps_missing_output_parent_to_open_output_failure() {
        let directory = tempfile::tempdir().expect("directory fixture should be created");
        let missing_parent_output = directory.path().join("missing-parent").join("output");
        let mut process = ProcessCommand::new("unused-test-program");
        let mut files = IoFiles::prepare(
            "command",
            CommandStdin::Null,
            Some(&missing_parent_output),
            None,
            &mut process,
        )
        .expect("an output under a missing parent is deferred until commit");

        let error = match files.commit("command", Some(&missing_parent_output), None, &mut process) {
            Err(error) => error,
            Ok(_) => panic!("commit cannot create an output whose parent is absent"),
        };

        assert_eq!(error.kind(), CommandErrorKind::OpenOutputFailed);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::OpenOutputFailed { stream, path, source }
                if *stream == OutputStream::Stdout
                    && path == &missing_parent_output
                    && source.kind() == io::ErrorKind::NotFound
        ));
        assert!(!missing_parent_output.exists());
    }

    #[test]
    fn test_prepare_rejects_non_regular_paths_and_input_output_aliases() {
        let directory = tempfile::tempdir().expect("directory fixture should be created");
        let mut process = ProcessCommand::new("unused-test-program");
        let error = IoFiles::prepare(
            "command",
            CommandStdin::File(directory.path().to_path_buf()),
            None,
            None,
            &mut process,
        )
        .err()
        .expect("directory cannot be configured as stdin");
        assert_eq!(error.kind(), crate::CommandErrorKind::NonRegularInputFile);

        let error = IoFiles::prepare(
            "command",
            CommandStdin::Null,
            Some(directory.path()),
            None,
            &mut process,
        )
        .err()
        .expect("directory cannot be configured as stdout");
        assert_eq!(error.kind(), crate::CommandErrorKind::NonRegularOutputFile);

        let input = tempfile::NamedTempFile::new().expect("input fixture should be created");
        std::fs::write(input.path(), b"preserve input").expect("input fixture should be populated");
        let error = IoFiles::prepare(
            "command",
            CommandStdin::File(input.path().to_path_buf()),
            Some(input.path()),
            None,
            &mut process,
        )
        .err()
        .expect("stdin and stdout cannot share a normalized path");
        assert_eq!(error.kind(), crate::CommandErrorKind::InputOutputConflict);
        assert_eq!(
            std::fs::read(input.path()).expect("input should remain readable"),
            b"preserve input",
        );
    }

    #[test]
    fn test_prepare_rejects_stdin_and_stderr_alias_without_changing_input() {
        let input = tempfile::NamedTempFile::new().expect("input fixture should be created");
        std::fs::write(input.path(), b"preserve input").expect("input fixture should be populated");
        let mut process = ProcessCommand::new("unused-test-program");

        let error = match IoFiles::prepare(
            "command",
            CommandStdin::File(input.path().to_path_buf()),
            None,
            Some(input.path()),
            &mut process,
        ) {
            Err(error) => error,
            Ok(_) => panic!("stdin and stderr cannot share one normalized path"),
        };

        assert_eq!(error.kind(), CommandErrorKind::InputOutputConflict);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::InputOutputConflict {
                input_path,
                output_stream,
                output_path,
            } if input_path == input.path()
                && *output_stream == OutputStream::Stderr
                && output_path == input.path()
        ));
        assert_eq!(
            std::fs::read(input.path()).expect("conflicting preparation must preserve input"),
            b"preserve input",
        );
    }

    #[test]
    fn test_prepare_rejects_relative_stdin_and_absolute_stdout_alias() {
        let working_directory = std::env::current_dir().expect("working directory should be available");
        let directory = tempfile::tempdir_in(&working_directory).expect("relative path fixture should be created");
        let input = directory.path().join("shared-input-output");
        std::fs::write(&input, b"preserve relative input").expect("input fixture should be populated");
        let relative_input = input
            .strip_prefix(&working_directory)
            .expect("fixture path should be under the current directory")
            .to_path_buf();
        let mut process = ProcessCommand::new("unused-test-program");

        let error = match IoFiles::prepare(
            "command",
            CommandStdin::File(relative_input.clone()),
            Some(&input),
            None,
            &mut process,
        ) {
            Err(error) => error,
            Ok(_) => {
                panic!("relative stdin and its absolute stdout path must be recognized as one file")
            }
        };

        assert_eq!(error.kind(), CommandErrorKind::InputOutputConflict);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::InputOutputConflict {
                input_path,
                output_stream,
                output_path,
            } if input_path == &relative_input
                && *output_stream == OutputStream::Stdout
                && output_path == &input
        ));
        assert_eq!(
            std::fs::read(&input).expect("conflicting relative input must remain readable"),
            b"preserve relative input",
        );
    }

    #[test]
    fn test_truncate_output_maps_injected_set_len_failure() {
        let output = tempfile::NamedTempFile::new().expect("output fixture should be created");
        std::fs::write(output.path(), b"preserve-on-failure").expect("output fixture should be populated");
        let file = OpenOptions::new()
            .write(true)
            .open(output.path())
            .expect("output fixture should be opened");

        let error = truncate_output_with(
            "command",
            OutputStream::Stdout,
            Some(output.path()),
            Some(&file),
            |_| Err(io::Error::other("injected truncate failure")),
        )
        .expect_err("injected truncation failure should be mapped");

        assert_eq!(error.kind(), CommandErrorKind::OpenOutputFailed);
        assert_eq!(
            std::fs::read(output.path()).expect("output fixture should remain readable"),
            b"preserve-on-failure",
        );
    }

    #[test]
    fn test_second_truncate_failure_leaves_stdout_empty_and_stderr_unchanged() {
        let stdout = tempfile::NamedTempFile::new().expect("stdout fixture should be created");
        let stderr = tempfile::NamedTempFile::new().expect("stderr fixture should be created");
        std::fs::write(stdout.path(), b"old stdout").expect("stdout should be populated");
        std::fs::write(stderr.path(), b"old stderr").expect("stderr should be populated");
        let stdout_file = OpenOptions::new()
            .write(true)
            .open(stdout.path())
            .expect("stdout should be opened");
        let stderr_file = OpenOptions::new()
            .write(true)
            .open(stderr.path())
            .expect("stderr should be opened");

        let error = truncate_outputs_with(
            "command",
            Some(stdout.path()),
            Some(&stdout_file),
            Some(stderr.path()),
            Some(&stderr_file),
            |command, stream, path, file| {
                if stream == OutputStream::Stderr {
                    truncate_output_with(command, stream, path, file, |_| {
                        Err(io::Error::other("injected stderr truncate failure"))
                    })
                } else {
                    super::truncate_output(command, stream, path, file)
                }
            },
        )
        .expect_err("injected stderr truncation should fail");

        assert_eq!(error.kind(), CommandErrorKind::OpenOutputFailed);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::OpenOutputFailed {
                stream: OutputStream::Stderr,
                ..
            }
        ));
        assert_eq!(std::fs::read(stdout.path()).expect("stdout should be readable"), b"",);
        assert_eq!(
            std::fs::read(stderr.path()).expect("stderr should be readable"),
            b"old stderr",
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_rejects_dangling_output_symlink_without_creating_target() {
        let directory = tempfile::tempdir().expect("directory fixture should be created");
        let target = directory.path().join("not-created");
        let link = directory.path().join("dangling-output-link");
        std::os::unix::fs::symlink(&target, &link).expect("dangling symlink should be created");
        let mut process = ProcessCommand::new("unused-test-program");

        let error = IoFiles::prepare("command", CommandStdin::Null, Some(&link), None, &mut process)
            .err()
            .expect("a dangling symlink is an existing non-regular output path");

        assert_eq!(error.kind(), CommandErrorKind::NonRegularOutputFile);
        assert!(std::fs::symlink_metadata(&link).is_ok());
        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_open_handle_checks_reject_directories_even_when_the_path_was_not_checked() {
        let directory = tempfile::tempdir().expect("directory fixture should be created");
        let file = std::fs::File::open(directory.path()).expect("Unix permits opening a directory handle");

        let input_error = ensure_regular_input_handle("command", directory.path(), &file)
            .expect_err("an opened directory is not a valid stdin file");
        assert_eq!(input_error.kind(), CommandErrorKind::NonRegularInputFile);

        let output_error = ensure_regular_output_handle("command", OutputStream::Stderr, directory.path(), &file)
            .expect_err("an opened directory is not a valid output file");
        assert_eq!(output_error.kind(), CommandErrorKind::NonRegularOutputFile);
        assert!(matches!(
            output_error.reason(),
            crate::CommandErrorReason::NonRegularOutputFile {
                stream: OutputStream::Stderr,
                ..
            }
        ));
    }

    #[test]
    fn test_prepare_maps_invalid_output_path_metadata_to_inspection_error() {
        let invalid_path = std::path::PathBuf::from("\0invalid-output-path");
        let mut process = ProcessCommand::new("unused-test-program");

        let error = IoFiles::prepare("command", CommandStdin::Null, Some(&invalid_path), None, &mut process)
            .err()
            .expect("OS rejection while inspecting a path should retain inspection context");

        assert_eq!(error.kind(), CommandErrorKind::InspectIoFileFailed);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::InspectIoFileFailed { path, source }
                if path == &invalid_path && source.kind() == io::ErrorKind::InvalidInput
        ));
    }
}
