// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable categories for command execution failures.

/// Stable, data-free category of a [`CommandError`](crate::CommandError).
///
/// # Examples
///
/// ```
/// use qubit_command::{Command, CommandErrorKind, CommandRunner};
///
/// let error = CommandRunner::without_timeout()
///     .run(Command::new("__qubit_command_example_missing_executable__"))
///     .expect_err("the example executable should not exist");
/// assert_eq!(error.kind(), CommandErrorKind::SpawnFailed);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use]
pub enum CommandErrorKind {
    /// The child process could not be spawned.
    SpawnFailed,
    /// Waiting for the child process failed.
    WaitFailed,
    /// Cancellation was requested before startup.
    CancelledBeforeStart,
    /// Termination after a timeout failed.
    KillFailed,
    /// Captured output could not be read.
    ReadOutputFailed,
    /// An input file could not be opened.
    OpenInputFailed,
    /// The configured input path is not a regular file.
    NonRegularInputFile,
    /// An output tee file could not be opened.
    OpenOutputFailed,
    /// The configured output path is not a regular file.
    NonRegularOutputFile,
    /// Input and output files conflict.
    InputOutputConflict,
    /// Output tee files conflict.
    OutputFilesConflict,
    /// An I/O file could not be inspected.
    InspectIoFileFailed,
    /// The stdin helper could not be started.
    StartInputThreadFailed,
    /// An output helper could not be started.
    StartOutputThreadFailed,
    /// Clock or timer handling failed.
    TimeFailed,
    /// Writing configured stdin failed.
    WriteInputFailed,
    /// Writing a captured stream to a tee failed.
    WriteOutputFailed,
    /// The command exceeded its timeout.
    TimedOut,
    /// The command was cancelled after startup.
    Cancelled,
    /// Process-tree cancellation failed.
    CancelFailed,
    /// Successful command output was truncated.
    OutputTruncated,
    /// The process exited with an unconfigured status.
    UnexpectedExit,
}
