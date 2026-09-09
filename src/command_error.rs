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
///
/// # Examples
///
/// ```
/// use qubit_command::{Command, CommandError, CommandRunner};
///
/// let error: CommandError = CommandRunner::without_timeout()
///     .run(Command::new("__qubit_command_example_missing_executable__"))
///     .expect_err("the example executable should not exist");
/// assert!(error.output().is_none());
/// ```
#[must_use]
pub struct CommandError {
    /// Human-readable, redacted command representation.
    command: String,
    /// Primary failure reason.
    reason: Box<CommandErrorReason>,
    /// Output retained before the primary failure, when available.
    output: Option<Box<CommandOutput>>,
    /// Failures observed while cleaning up after the primary failure.
    cleanup_failures: Vec<CommandCleanupFailure>,
}

const _: () = assert!(std::mem::size_of::<CommandError>() <= 96);

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
            reason: Box::new(reason),
            output,
            cleanup_failures: Vec::new(),
        }
    }

    /// Adds cleanup failures and restores canonical resource order.
    ///
    /// Failures are ordered by process tree, direct child, wait, time, stdout,
    /// stderr, and stdin. Relative order remains stable within each resource.
    #[inline]
    pub(crate) fn with_cleanup_failures(
        mut self,
        cleanup_failures: impl IntoIterator<Item = CommandCleanupFailure>,
    ) -> Self {
        self.cleanup_failures.extend(cleanup_failures);
        self.cleanup_failures.sort_by_key(cleanup_failure_rank);
        self
    }

    /// Attaches captured output without changing the primary reason or source.
    #[inline]
    pub(crate) fn with_output(mut self, output: CommandOutput) -> Self {
        self.output = Some(Box::new(output));
        self
    }

    /// Converts a helper error into its cleanup representation.
    pub(crate) fn into_cleanup_failure(self) -> Option<CommandCleanupFailure> {
        match *self.reason {
            CommandErrorReason::TimeFailed { source } => Some(CommandCleanupFailure::Time { source }),
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

    /// Converts this error and its cleanup details into cleanup failures.
    ///
    /// The primary reason is included first when it represents an I/O helper
    /// failure. Existing cleanup failures retain their original order.
    pub(crate) fn into_cleanup_failures(self) -> Vec<CommandCleanupFailure> {
        let Self {
            reason,
            cleanup_failures,
            ..
        } = self;
        let primary = match *reason {
            CommandErrorReason::TimeFailed { source } => Some(CommandCleanupFailure::Time { source }),
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
        };
        primary.into_iter().chain(cleanup_failures).collect()
    }

    /// Merges a finalization error without changing the selected primary
    /// reason.
    ///
    /// Moves retained output when this error has none and records helper/time
    /// failures as cleanup evidence. The input must be a finalization error.
    pub(crate) fn with_finalization_error(mut self, mut error: CommandError) -> Self {
        if self.output.is_none() {
            self.output = error.output.take();
        }
        self.with_cleanup_failures(error.into_cleanup_failures())
    }

    /// Returns the redacted command representation.
    ///
    /// # Returns
    ///
    /// The command text with sensitive arguments, environment values, and
    /// paths removed or masked.
    #[must_use]
    #[inline(always)]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Returns the stable, data-free error category.
    ///
    /// # Returns
    ///
    /// The category corresponding to the primary failure reason.
    #[inline(always)]
    pub fn kind(&self) -> CommandErrorKind {
        self.reason.as_ref().into()
    }

    /// Returns the detailed primary failure reason.
    ///
    /// # Returns
    ///
    /// The structured reason selected for this error.
    #[inline(always)]
    pub fn reason(&self) -> &CommandErrorReason {
        self.reason.as_ref()
    }

    /// Returns captured output retained by the primary failure.
    ///
    /// # Returns
    ///
    /// `Some(output)` when the failure retained partial or complete output;
    /// otherwise `None`.
    #[must_use]
    #[inline(always)]
    pub fn output(&self) -> Option<&CommandOutput> {
        self.output.as_deref()
    }

    /// Consumes the error and returns captured output, when available.
    ///
    /// # Returns
    ///
    /// The retained output, or `None` when no output was available.
    #[must_use]
    #[inline(always)]
    pub fn into_output(self) -> Option<CommandOutput> {
        self.output.map(|output| *output)
    }

    /// Returns every cleanup failure observed after the primary failure.
    ///
    /// # Returns
    ///
    /// Cleanup failures in canonical resource order.
    #[inline(always)]
    pub fn cleanup_failures(&self) -> &[CommandCleanupFailure] {
        &self.cleanup_failures
    }

    /// Returns the process exit code when one was observed.
    ///
    /// # Returns
    ///
    /// `Some(code)` when a numeric exit code was observed, otherwise `None`.
    #[must_use]
    #[inline]
    pub fn exit_code(&self) -> Option<i32> {
        match self.reason.as_ref() {
            CommandErrorReason::UnexpectedExit { exit_code, .. } => *exit_code,
            _ => self.output.as_deref().and_then(CommandOutput::exit_code),
        }
    }

    /// Returns whether this is an unexpected process exit.
    ///
    /// # Returns
    ///
    /// `true` when the primary reason is [`CommandErrorKind::UnexpectedExit`].
    #[must_use]
    #[inline(always)]
    pub fn is_unexpected_exit(&self) -> bool {
        matches!(self.kind(), CommandErrorKind::UnexpectedExit)
    }

    /// Returns the first process-tree termination source from cleanup failures.
    ///
    /// # Returns
    ///
    /// The first process-tree termination error, or `None` when cleanup did
    /// not report one.
    #[must_use]
    pub fn process_tree_source(&self) -> Option<&io::Error> {
        self.cleanup_failures.iter().find_map(|failure| match failure {
            CommandCleanupFailure::ProcessTreeTermination { source } => Some(source),
            _ => None,
        })
    }

    /// Returns the first direct-child termination source from cleanup
    /// failures.
    ///
    /// # Returns
    ///
    /// The first direct-child termination error, or `None` when cleanup did
    /// not report one.
    #[must_use]
    pub fn child_source(&self) -> Option<&io::Error> {
        self.cleanup_failures.iter().find_map(|failure| match failure {
            CommandCleanupFailure::ChildTermination { source } => Some(source),
            _ => None,
        })
    }
}

/// Returns the canonical cleanup resource rank.
const fn cleanup_failure_rank(failure: &CommandCleanupFailure) -> u8 {
    match failure {
        CommandCleanupFailure::ProcessTreeTermination { .. } => 0,
        CommandCleanupFailure::ChildTermination { .. } => 1,
        CommandCleanupFailure::Wait { .. } => 2,
        CommandCleanupFailure::Time { .. } => 3,
        CommandCleanupFailure::StdoutCancellation { .. }
        | CommandCleanupFailure::StdoutRead { .. }
        | CommandCleanupFailure::StdoutWrite { .. } => 4,
        CommandCleanupFailure::StderrCancellation { .. }
        | CommandCleanupFailure::StderrRead { .. }
        | CommandCleanupFailure::StderrWrite { .. } => 5,
        CommandCleanupFailure::Stdin { .. } | CommandCleanupFailure::StdinCancellation { .. } => 6,
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let command = &self.command;
        match (self.reason.as_ref(), self.output.as_deref()) {
            (CommandErrorReason::SpawnFailed { source }, _) => {
                write!(formatter, "failed to spawn command `{command}`: {source}")
            }
            (CommandErrorReason::WaitFailed { source }, _) => {
                write!(formatter, "failed to wait for command `{command}`: {source}")
            }
            (CommandErrorReason::CancelledBeforeStart, _) => {
                write!(formatter, "command `{command}` was cancelled before it started")
            }
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
        match self.reason.as_ref() {
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
    #[cfg(not(unix))]
    let _ = output;
    format!("code {exit_code:?}")
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::process::ExitStatus;
    use std::time::Duration;

    use qubit_clock::TimeError;

    use super::CommandError;
    use crate::CommandCleanupFailure;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;
    use crate::CommandOutput;
    use crate::OutputStream;

    /// Creates a portable successful exit status for error assembly tests.
    fn status() -> ExitStatus {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        }
    }

    #[test]
    fn test_finalization_preserves_timeout_and_partial_output() {
        let output = CommandOutput::new(
            status(),
            (b"prefix".to_vec(), false, false),
            (Vec::new(), false, true),
            Duration::from_secs(1),
        );
        let secondary = CommandError::from_reason(
            "command",
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stdout,
                source: io::Error::other("read failed"),
            },
            Some(Box::new(output)),
        );
        let primary = CommandError::from_reason(
            "command",
            CommandErrorReason::TimedOut {
                timeout: Duration::from_secs(1),
            },
            None,
        );
        let error = primary.with_finalization_error(secondary);
        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        let output = error
            .output()
            .expect("partial output must survive finalization failure");
        assert_eq!(output.stdout(), b"prefix");
        assert!(!output.stdout_complete());
        assert!(matches!(
            error.cleanup_failures(),
            [CommandCleanupFailure::StdoutRead { .. }]
        ));
    }

    #[test]
    fn test_finalization_retains_time_failure_after_cancellation() {
        let secondary = CommandError::from_reason(
            "command",
            CommandErrorReason::TimeFailed {
                source: TimeError::InstantOverflow,
            },
            None,
        )
        .with_cleanup_failures([CommandCleanupFailure::Stdin {
            source: io::Error::other("stdin failed"),
        }]);
        let primary = CommandError::from_reason("command", CommandErrorReason::Cancelled, None);
        let error = primary.with_finalization_error(secondary);
        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        assert!(error.output().is_none());
        assert!(matches!(
            error.cleanup_failures(),
            [CommandCleanupFailure::Time { .. }, CommandCleanupFailure::Stdin { .. }]
        ));
    }

    #[test]
    fn test_finalization_keeps_existing_primary_output() {
        let primary_output = CommandOutput::new(
            status(),
            (b"primary".to_vec(), false, true),
            (Vec::new(), false, true),
            Duration::from_secs(2),
        );
        let secondary_output = CommandOutput::new(
            status(),
            (b"secondary".to_vec(), false, false),
            (Vec::new(), false, true),
            Duration::from_secs(3),
        );
        let primary =
            CommandError::from_reason("command", CommandErrorReason::Cancelled, Some(Box::new(primary_output)));
        let secondary = CommandError::from_reason(
            "command",
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stdout,
                source: io::Error::other("read failed"),
            },
            Some(Box::new(secondary_output)),
        );
        let error = primary.with_finalization_error(secondary);
        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        let output = error.output().expect("primary output must be retained");
        assert_eq!(output.stdout(), b"primary");
        assert!(output.stdout_complete());
        assert_eq!(output.elapsed(), Duration::from_secs(2));
        assert!(matches!(
            error.cleanup_failures(),
            [CommandCleanupFailure::StdoutRead { .. }]
        ));
    }
}
