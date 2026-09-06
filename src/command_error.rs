// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public command execution error container.

use std::error::Error;
use std::fmt;
use std::io;

use crate::CommandCleanupFailure;
use crate::CommandErrorKind;
use crate::CommandErrorReason;
use crate::CommandOutput;
use crate::OutputStream;

/// Error returned while preparing, spawning, waiting for, or collecting a
/// command.
pub struct CommandError {
    /// Human-readable, redacted command representation.
    command: String,
    /// Primary failure reason.
    reason: CommandErrorReason,
    /// Output retained before the primary failure, when available.
    output: Option<Box<CommandOutput>>,
    /// Failures observed while cleaning up after the primary failure.
    cleanup_failures: Vec<CommandCleanupFailure>,
}

impl fmt::Debug for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandError")
            .field("message", &self.to_string())
            .finish()
    }
}

impl CommandError {
    /// Creates an error from a primary reason and optional captured output.
    #[inline]
    pub(crate) fn from_reason(
        command: impl Into<String>,
        reason: CommandErrorReason,
        output: Option<Box<CommandOutput>>,
    ) -> Self {
        Self {
            command: command.into(),
            reason,
            output,
            cleanup_failures: Vec::new(),
        }
    }

    /// Adds all cleanup failures observed after the primary error.
    #[inline]
    pub(crate) fn with_cleanup_failures(
        mut self,
        cleanup_failures: impl IntoIterator<Item = CommandCleanupFailure>,
    ) -> Self {
        self.cleanup_failures.extend(cleanup_failures);
        self
    }

    /// Converts a helper error into its cleanup representation.
    pub(crate) fn into_cleanup_failure(self) -> Option<CommandCleanupFailure> {
        match self.reason {
            CommandErrorReason::WriteInputFailed { source } => Some(CommandCleanupFailure::Stdin { source }),
            CommandErrorReason::ReadOutputFailed { stream, source } => match stream {
                OutputStream::Stdout => Some(CommandCleanupFailure::StdoutRead { source }),
                OutputStream::Stderr => Some(CommandCleanupFailure::StderrRead { source }),
            },
            CommandErrorReason::WriteOutputFailed { stream, path, source } => match stream {
                OutputStream::Stdout => Some(CommandCleanupFailure::StdoutWrite { path, source }),
                OutputStream::Stderr => Some(CommandCleanupFailure::StderrWrite { path, source }),
            },
            _ => None,
        }
    }

    /// Returns the redacted command representation.
    #[must_use]
    #[inline(always)]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Returns the stable, data-free error category.
    #[must_use]
    #[inline(always)]
    pub fn kind(&self) -> CommandErrorKind {
        (&self.reason).into()
    }

    /// Returns the detailed primary failure reason.
    #[must_use]
    #[inline(always)]
    pub fn reason(&self) -> &CommandErrorReason {
        &self.reason
    }

    /// Returns captured output retained by the primary failure.
    #[must_use]
    #[inline(always)]
    pub fn output(&self) -> Option<&CommandOutput> {
        self.output.as_deref()
    }

    /// Consumes the error and returns captured output, when available.
    #[must_use]
    #[inline(always)]
    pub fn into_output(self) -> Option<CommandOutput> {
        self.output.map(|output| *output)
    }

    /// Returns every cleanup failure observed after the primary failure.
    #[must_use]
    #[inline(always)]
    pub fn cleanup_failures(&self) -> &[CommandCleanupFailure] {
        &self.cleanup_failures
    }

    /// Returns the process exit code when one was observed.
    #[must_use]
    #[inline]
    pub fn exit_code(&self) -> Option<i32> {
        match &self.reason {
            CommandErrorReason::UnexpectedExit { exit_code, .. } => *exit_code,
            _ => self.output.as_deref().and_then(CommandOutput::exit_code),
        }
    }

    /// Returns whether this is an unexpected process exit.
    #[must_use]
    #[inline(always)]
    pub fn is_unexpected_exit(&self) -> bool {
        matches!(self.kind(), CommandErrorKind::UnexpectedExit)
    }

    /// Returns the process-tree source from the primary or cleanup failures.
    #[must_use]
    pub fn process_tree_source(&self) -> Option<&io::Error> {
        if let CommandErrorReason::KillFailed {
            process_tree_source, ..
        }
        | CommandErrorReason::CancelFailed {
            process_tree_source, ..
        } = &self.reason
        {
            return Some(process_tree_source);
        }
        self.cleanup_failures.iter().find_map(|failure| match failure {
            CommandCleanupFailure::ProcessTreeTermination { source } => Some(source),
            _ => None,
        })
    }

    /// Returns the direct-child termination source from the primary or cleanup
    /// failures.
    #[must_use]
    pub fn child_source(&self) -> Option<&io::Error> {
        if let CommandErrorReason::KillFailed { child_source, .. }
        | CommandErrorReason::CancelFailed { child_source, .. } = &self.reason
        {
            return Some(child_source);
        }
        self.cleanup_failures.iter().find_map(|failure| match failure {
            CommandCleanupFailure::ChildTermination { source } => Some(source),
            _ => None,
        })
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let command = &self.command;
        match (&self.reason, self.output.as_deref()) {
            (CommandErrorReason::SpawnFailed { source }, _) => {
                write!(formatter, "failed to spawn command `{command}`: {source}")
            }
            (CommandErrorReason::WaitFailed { source }, _) => {
                write!(formatter, "failed to wait for command `{command}`: {source}")
            }
            (CommandErrorReason::CancelledBeforeStart, _) => {
                write!(formatter, "command `{command}` was cancelled before it started")
            }
            (
                CommandErrorReason::KillFailed {
                    timeout,
                    process_tree_source,
                    child_source,
                },
                _,
            ) => write!(
                formatter,
                "failed to terminate timed-out command `{command}` after {timeout:?}; process-tree source: {process_tree_source}; child source: {child_source}"
            ),
            (CommandErrorReason::ReadOutputFailed { stream, source }, _) => {
                write!(formatter, "failed to read {stream} for command `{command}`: {source}")
            }
            (CommandErrorReason::OpenInputFailed { source, .. }, _) => write!(
                formatter,
                "failed to open stdin file `<redacted path>` for command `{command}`: {source}"
            ),
            (CommandErrorReason::NonRegularInputFile { .. }, _) => write!(
                formatter,
                "stdin path `<redacted path>` for command `{command}` is not an ordinary file"
            ),
            (CommandErrorReason::OpenOutputFailed { stream, source, .. }, _) => write!(
                formatter,
                "failed to open {stream} file `<redacted path>` for command `{command}`: {source}"
            ),
            (CommandErrorReason::NonRegularOutputFile { stream, .. }, _) => {
                write!(
                    formatter,
                    "{stream} path `<redacted path>` for command `{command}` is not an ordinary file"
                )
            }
            (CommandErrorReason::InputOutputConflict { output_stream, .. }, _) => write!(
                formatter,
                "stdin file '<redacted path>' conflicts with {output_stream} file '<redacted path>' for command '{command}'"
            ),
            (CommandErrorReason::OutputFilesConflict { .. }, _) => write!(
                formatter,
                "stdout file '<redacted path>' conflicts with stderr file '<redacted path>' for command '{command}'"
            ),
            (CommandErrorReason::InspectIoFileFailed { source, .. }, _) => {
                write!(
                    formatter,
                    "failed to inspect I/O file '<redacted path>' for command '{command}': {source}"
                )
            }
            (CommandErrorReason::StartInputThreadFailed { source }, _) => {
                write!(
                    formatter,
                    "failed to start stdin writer for command '{command}': {source}"
                )
            }
            (CommandErrorReason::StartOutputThreadFailed { stream, source }, _) => write!(
                formatter,
                "failed to start {stream} reader for command '{command}': {source}"
            ),
            (CommandErrorReason::TimeFailed { source }, _) => {
                write!(formatter, "time handling failed for command '{command}': {source}")
            }
            (CommandErrorReason::WriteInputFailed { source }, _) => {
                write!(formatter, "failed to write stdin for command `{command}`: {source}")
            }
            (CommandErrorReason::WriteOutputFailed { stream, source, .. }, _) => write!(
                formatter,
                "failed to write {stream} for command `{command}` to `<redacted path>`: {source}"
            ),
            (CommandErrorReason::TimedOut { timeout }, _) => {
                write!(formatter, "command `{command}` timed out after {timeout:?}")
            }
            (CommandErrorReason::Cancelled, _) => {
                write!(formatter, "command `{command}` was cancelled")
            }
            (
                CommandErrorReason::CancelFailed {
                    process_tree_source,
                    child_source,
                },
                _,
            ) => write!(
                formatter,
                "failed to cancel command `{command}`; process-tree source: {process_tree_source}; child source: {child_source}"
            ),
            (CommandErrorReason::OutputTruncated, _) => write!(
                formatter,
                "command `{command}` completed successfully, but captured output was truncated"
            ),
            (CommandErrorReason::UnexpectedExit { exit_code, expected }, output) => write!(
                formatter,
                "command `{command}` exited with {}; expected one of {expected:?}",
                unexpected_exit_detail(exit_code, output),
            ),
        }?;
        if !self.cleanup_failures.is_empty() {
            write!(formatter, "; {} cleanup failure(s)", self.cleanup_failures.len())?;
        }
        Ok(())
    }
}

impl Error for CommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.reason {
            CommandErrorReason::SpawnFailed { source }
            | CommandErrorReason::WaitFailed { source }
            | CommandErrorReason::ReadOutputFailed { source, .. }
            | CommandErrorReason::OpenInputFailed { source, .. }
            | CommandErrorReason::OpenOutputFailed { source, .. }
            | CommandErrorReason::InspectIoFileFailed { source, .. }
            | CommandErrorReason::StartInputThreadFailed { source }
            | CommandErrorReason::StartOutputThreadFailed { source, .. }
            | CommandErrorReason::WriteInputFailed { source }
            | CommandErrorReason::WriteOutputFailed { source, .. } => Some(source),
            CommandErrorReason::KillFailed {
                process_tree_source, ..
            }
            | CommandErrorReason::CancelFailed {
                process_tree_source, ..
            } => Some(process_tree_source),
            CommandErrorReason::TimeFailed { source } => Some(source),
            _ => None,
        }
    }
}

impl From<&CommandErrorReason> for CommandErrorKind {
    fn from(reason: &CommandErrorReason) -> Self {
        match reason {
            CommandErrorReason::SpawnFailed { .. } => Self::SpawnFailed,
            CommandErrorReason::WaitFailed { .. } => Self::WaitFailed,
            CommandErrorReason::CancelledBeforeStart => Self::CancelledBeforeStart,
            CommandErrorReason::KillFailed { .. } => Self::KillFailed,
            CommandErrorReason::ReadOutputFailed { .. } => Self::ReadOutputFailed,
            CommandErrorReason::OpenInputFailed { .. } => Self::OpenInputFailed,
            CommandErrorReason::NonRegularInputFile { .. } => Self::NonRegularInputFile,
            CommandErrorReason::OpenOutputFailed { .. } => Self::OpenOutputFailed,
            CommandErrorReason::NonRegularOutputFile { .. } => Self::NonRegularOutputFile,
            CommandErrorReason::InputOutputConflict { .. } => Self::InputOutputConflict,
            CommandErrorReason::OutputFilesConflict { .. } => Self::OutputFilesConflict,
            CommandErrorReason::InspectIoFileFailed { .. } => Self::InspectIoFileFailed,
            CommandErrorReason::StartInputThreadFailed { .. } => Self::StartInputThreadFailed,
            CommandErrorReason::StartOutputThreadFailed { .. } => Self::StartOutputThreadFailed,
            CommandErrorReason::TimeFailed { .. } => Self::TimeFailed,
            CommandErrorReason::WriteInputFailed { .. } => Self::WriteInputFailed,
            CommandErrorReason::WriteOutputFailed { .. } => Self::WriteOutputFailed,
            CommandErrorReason::TimedOut { .. } => Self::TimedOut,
            CommandErrorReason::Cancelled => Self::Cancelled,
            CommandErrorReason::CancelFailed { .. } => Self::CancelFailed,
            CommandErrorReason::OutputTruncated => Self::OutputTruncated,
            CommandErrorReason::UnexpectedExit { .. } => Self::UnexpectedExit,
        }
    }
}

/// Formats the observed termination detail for an unexpected command exit.
fn unexpected_exit_detail(exit_code: &Option<i32>, output: Option<&CommandOutput>) -> String {
    #[cfg(unix)]
    if let Some(signal) = output.and_then(CommandOutput::termination_signal) {
        return format!("signal {signal}");
    }
    format!("code {exit_code:?}")
}
