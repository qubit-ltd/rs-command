// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    fmt,
    io,
    path::PathBuf,
    time::Duration,
};

use thiserror::Error;

use qubit_clock::TimeError;

use crate::{
    CommandOutput,
    OutputStream,
};

/// Error returned while preparing, spawning, waiting for, or collecting a
/// command.
///
/// This enum is non-exhaustive; downstream matches must retain a wildcard arm
/// so future process and platform failures can be represented without another
/// breaking change.
#[derive(Error)]
#[non_exhaustive]
pub enum CommandError {
    /// The process could not be spawned.
    #[error("failed to spawn command `{command}`: {source}")]
    SpawnFailed {
        /// Human-readable command representation.
        command: String,
        /// I/O error reported by the operating system.
        source: io::Error,
    },

    /// Waiting for process completion failed.
    #[error("failed to wait for command `{command}`: {source}")]
    WaitFailed {
        /// Human-readable command representation.
        command: String,
        /// I/O error reported while waiting for the child process.
        source: io::Error,
    },

    /// Cancellation was requested before the command was prepared or started.
    #[error("command `{command}` was cancelled before it started")]
    CancelledBeforeStart {
        /// Human-readable command representation.
        command: String,
    },

    /// The process could not be killed after exceeding the configured timeout.
    #[error(
        "failed to terminate timed-out command `{command}` after {timeout:?}; process-tree source: {process_tree_source}; child source: {child_source}"
    )]
    KillFailed {
        /// Human-readable command representation.
        command: String,
        /// Timeout that was exceeded.
        timeout: Duration,
        /// I/O error reported while killing the process tree.
        #[source]
        process_tree_source: io::Error,
        /// I/O error reported while killing the direct child process.
        child_source: io::Error,
    },

    /// Reading one of the captured output streams failed.
    #[error("failed to read {stream} for command `{command}`: {source}")]
    ReadOutputFailed {
        /// Human-readable command representation.
        command: String,
        /// Stream whose reader failed.
        stream: OutputStream,
        /// I/O error reported while reading the stream.
        source: io::Error,
    },

    /// Opening a stdin file failed.
    #[error(
        "failed to open stdin file `<redacted path>` for command `{command}`: {source}"
    )]
    OpenInputFailed {
        /// Human-readable command representation.
        command: String,
        /// Path that could not be opened.
        path: PathBuf,
        /// I/O error reported while opening the file.
        source: io::Error,
    },

    /// The configured stdin path does not identify an ordinary file.
    #[error(
        "stdin path `<redacted path>` for command `{command}` is not an ordinary file"
    )]
    NonRegularInputFile {
        /// Human-readable command representation.
        command: String,
        /// Configured stdin path.
        path: PathBuf,
    },

    /// Opening an output redirection file failed.
    #[error(
        "failed to open {stream} file `<redacted path>` for command `{command}`: {source}"
    )]
    OpenOutputFailed {
        /// Human-readable command representation.
        command: String,
        /// Stream whose file could not be opened.
        stream: OutputStream,
        /// Path that could not be opened.
        path: PathBuf,
        /// I/O error reported while opening the file.
        source: io::Error,
    },

    /// The configured output tee path does not identify an ordinary file.
    #[error(
        "{stream} path `<redacted path>` for command `{command}` is not an ordinary file"
    )]
    NonRegularOutputFile {
        /// Human-readable command representation.
        command: String,
        /// Output stream receiving the tee.
        stream: OutputStream,
        /// Configured output tee path.
        path: PathBuf,
    },

    /// An input file and one output tee identify the same file.
    #[error(
        "stdin file '<redacted path>' conflicts with {output_stream} file '<redacted path>' for command '{command}'"
    )]
    InputOutputConflict {
        /// Human-readable command representation.
        command: String,
        /// Configured stdin file path.
        input_path: PathBuf,
        /// Output stream whose tee file conflicts with stdin.
        output_stream: OutputStream,
        /// Configured output tee path.
        output_path: PathBuf,
    },

    /// Stdout and stderr tee paths identify the same file.
    #[error(
        "stdout file '<redacted path>' conflicts with stderr file '<redacted path>' for command '{command}'"
    )]
    OutputFilesConflict {
        /// Human-readable command representation.
        command: String,
        /// Configured stdout tee path.
        stdout_path: PathBuf,
        /// Configured stderr tee path.
        stderr_path: PathBuf,
    },

    /// Inspecting an I/O file for conflict detection failed.
    #[error(
        "failed to inspect I/O file '<redacted path>' for command '{command}': {source}"
    )]
    InspectIoFileFailed {
        /// Human-readable command representation.
        command: String,
        /// Configured I/O file path.
        path: PathBuf,
        /// I/O error reported while resolving or inspecting the file.
        source: io::Error,
    },

    /// Starting the helper thread that writes stdin failed.
    #[error("failed to start stdin writer for command '{command}': {source}")]
    StartInputThreadFailed {
        /// Human-readable command representation.
        command: String,
        /// I/O error reported while creating the helper thread.
        source: io::Error,
    },

    /// Starting a helper thread that reads captured output failed.
    #[error(
        "failed to start {stream} reader for command '{command}': {source}"
    )]
    StartOutputThreadFailed {
        /// Human-readable command representation.
        command: String,
        /// Output stream whose helper thread could not be started.
        stream: OutputStream,
        /// I/O error reported while creating the helper thread.
        source: io::Error,
    },

    /// Monotonic time measurement or sleeping failed.
    #[error("time handling failed for command '{command}': {source}")]
    TimeFailed {
        /// Human-readable command representation.
        command: String,
        /// Timer or monotonic-clock error.
        source: TimeError,
    },

    /// Writing configured stdin bytes failed.
    #[error("failed to write stdin for command `{command}`: {source}")]
    WriteInputFailed {
        /// Human-readable command representation.
        command: String,
        /// I/O error reported while writing stdin.
        source: io::Error,
    },

    /// Writing captured output to a redirection file failed.
    #[error(
        "failed to write {stream} for command `{command}` to `<redacted path>`: {source}"
    )]
    WriteOutputFailed {
        /// Human-readable command representation.
        command: String,
        /// Stream whose redirected writer failed.
        stream: OutputStream,
        /// Path that could not be written.
        path: PathBuf,
        /// I/O error reported while writing the file.
        source: io::Error,
        /// Output retained while draining the stream after the tee writer
        /// failed, when available.
        output: Option<Box<CommandOutput>>,
    },

    /// The command exceeded the configured timeout and was terminated.
    #[error("command `{command}` timed out after {timeout:?}")]
    TimedOut {
        /// Human-readable command representation.
        command: String,
        /// Timeout that was exceeded.
        timeout: Duration,
        /// Output captured before the terminated process and I/O cleanup
        /// cutoff. It can omit bytes held by escaped descendants or blocked
        /// output helpers.
        output: Box<CommandOutput>,
    },

    /// The command was cancelled and its managed process tree was terminated.
    #[error("command `{command}` was cancelled")]
    Cancelled {
        /// Human-readable command representation.
        command: String,
        /// Output captured before the terminated process and I/O cleanup
        /// cutoff. It can omit bytes held by escaped descendants or blocked
        /// output helpers.
        output: Box<CommandOutput>,
    },

    /// Cancelling the managed process tree failed.
    #[error(
        "failed to cancel command `{command}`; process-tree source: {process_tree_source}; child source: {child_source}"
    )]
    CancelFailed {
        /// Human-readable command representation.
        command: String,
        /// I/O error reported while cancelling the process tree.
        #[source]
        process_tree_source: io::Error,
        /// I/O error reported while cancelling the direct child process.
        child_source: io::Error,
    },

    /// The command succeeded, but its captured output was truncated.
    #[error(
        "command `{command}` completed successfully, but captured output was truncated"
    )]
    OutputTruncated {
        /// Human-readable command representation.
        command: String,
        /// Partially retained output from the successful command.
        output: Box<CommandOutput>,
    },

    /// The command completed with a status not configured as successful.
    #[error(
        "command `{command}` exited with {}; expected one of {expected:?}",
        unexpected_exit_detail(.exit_code, .output.as_ref())
    )]
    UnexpectedExit {
        /// Human-readable command representation.
        command: String,
        /// Exit code reported by the process, if available.
        exit_code: Option<i32>,
        /// Configured successful exit codes.
        expected: Vec<i32>,
        /// Captured output from the failed command.
        output: Box<CommandOutput>,
    },
}

/// Formats the observed termination detail for an unexpected command exit.
///
/// # Parameters
///
/// * `exit_code` - Numeric exit code reported by the process, if available.
/// * `output` - Captured command status and output metadata.
///
/// # Returns
///
/// A diagnostic that reports the Unix termination signal when available, or
/// preserves the numeric exit-code representation on other platforms.
fn unexpected_exit_detail(
    exit_code: &Option<i32>,
    output: &CommandOutput,
) -> String {
    #[cfg(unix)]
    if let Some(signal) = output.termination_signal() {
        return format!("signal {signal}");
    }
    format!("code {exit_code:?}")
}

impl fmt::Debug for CommandError {
    /// Formats the error without exposing retained I/O path fields.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Destination formatter.
    ///
    /// # Returns
    ///
    /// Formatting result after rendering the redacted error message.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandError")
            .field("message", &self.to_string())
            .finish()
    }
}

impl CommandError {
    /// Returns captured command output when this error carries it.
    ///
    /// # Returns
    ///
    /// `Some(output)` for timeout, cancellation, output-truncation,
    /// unexpected-exit, and tee write errors that retained output; otherwise
    /// `None`.
    #[inline(always)]
    pub const fn output(&self) -> Option<&CommandOutput> {
        match self {
            Self::TimedOut { output, .. }
            | Self::Cancelled { output, .. }
            | Self::OutputTruncated { output, .. }
            | Self::UnexpectedExit { output, .. } => Some(output),
            Self::WriteOutputFailed {
                output: Some(output),
                ..
            } => Some(output),
            _ => None,
        }
    }

    /// Consumes this error and returns captured output when it is available.
    ///
    /// # Returns
    ///
    /// `Some(output)` for timeout, cancellation, output-truncation,
    /// unexpected-exit, and tee write errors that retained output; otherwise
    /// `None`.
    #[must_use]
    #[inline(always)]
    pub fn into_output(self) -> Option<CommandOutput> {
        match self {
            Self::TimedOut { output, .. }
            | Self::Cancelled { output, .. }
            | Self::OutputTruncated { output, .. }
            | Self::UnexpectedExit { output, .. } => Some(*output),
            Self::WriteOutputFailed {
                output: Some(output),
                ..
            } => Some(*output),
            _ => None,
        }
    }

    /// Returns the command string associated with this error.
    ///
    /// # Returns
    ///
    /// A human-readable command representation used in diagnostics.
    #[must_use]
    #[inline(always)]
    pub fn command(&self) -> &str {
        match self {
            Self::SpawnFailed { command, .. }
            | Self::WaitFailed { command, .. }
            | Self::CancelledBeforeStart { command, .. }
            | Self::KillFailed { command, .. }
            | Self::CancelFailed { command, .. }
            | Self::ReadOutputFailed { command, .. }
            | Self::OpenInputFailed { command, .. }
            | Self::NonRegularInputFile { command, .. }
            | Self::OpenOutputFailed { command, .. }
            | Self::NonRegularOutputFile { command, .. }
            | Self::InputOutputConflict { command, .. }
            | Self::OutputFilesConflict { command, .. }
            | Self::InspectIoFileFailed { command, .. }
            | Self::StartInputThreadFailed { command, .. }
            | Self::StartOutputThreadFailed { command, .. }
            | Self::TimeFailed { command, .. }
            | Self::WriteInputFailed { command, .. }
            | Self::WriteOutputFailed { command, .. }
            | Self::TimedOut { command, .. }
            | Self::Cancelled { command, .. }
            | Self::OutputTruncated { command, .. }
            | Self::UnexpectedExit { command, .. } => command,
        }
    }

    /// Returns the process-tree failure carried by a termination error.
    ///
    /// # Returns
    ///
    /// `Some(source)` for [`Self::KillFailed`] and [`Self::CancelFailed`], or
    /// `None` for errors that do not represent a two-stage termination failure.
    #[must_use]
    #[inline(always)]
    pub fn process_tree_source(&self) -> Option<&io::Error> {
        match self {
            Self::KillFailed {
                process_tree_source,
                ..
            }
            | Self::CancelFailed {
                process_tree_source,
                ..
            } => Some(process_tree_source),
            _ => None,
        }
    }

    /// Returns the direct-child failure carried by a termination error.
    ///
    /// # Returns
    ///
    /// `Some(source)` for [`Self::KillFailed`] and [`Self::CancelFailed`], or
    /// `None` for errors that do not represent a two-stage termination failure.
    #[must_use]
    #[inline(always)]
    pub fn child_source(&self) -> Option<&io::Error> {
        match self {
            Self::KillFailed { child_source, .. }
            | Self::CancelFailed { child_source, .. } => Some(child_source),
            _ => None,
        }
    }
}
