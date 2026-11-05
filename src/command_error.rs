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

use crate::CommandCapturedOutput;
use crate::CommandCleanupFailure;
use crate::CommandErrorKind;
use crate::CommandErrorReason;
use crate::CommandOutput;
use crate::OutputStream;
use crate::internal::diagnostic_source::TIME_SOURCE_SUMMARY;
use crate::internal::diagnostic_source::io_source_summary;

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
    /// Captured bytes when no real process status is available.
    captured_output: Option<Box<CommandCapturedOutput>>,
    /// Failures observed while cleaning up after the primary failure.
    cleanup_failures: Vec<CommandCleanupFailure>,
}

const _: () = assert!(std::mem::size_of::<CommandError>() <= 96);

impl fmt::Debug for CommandError {
    /// Formats a diagnostic summary without exposing the command's raw sources.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Formatter receiving the redacted error summary.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandError")
            .field("message", &self.to_string())
            .finish()
    }
}

impl CommandError {
    /// Creates an error from a primary reason and optional captured output.
    ///
    /// # Parameters
    ///
    /// * `command` - Redacted command text associated with the failure.
    /// * `reason` - Primary failure reason.
    /// * `output` - Optional output retained with a real process result.
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
            captured_output: None,
            cleanup_failures: Vec::new(),
        }
    }

    /// Adds cleanup failures and restores canonical resource order.
    ///
    /// Failures are ordered by process tree, direct child, wait, time, stdout,
    /// stderr, and stdin. Relative order remains stable within each resource.
    ///
    /// # Parameters
    ///
    /// * `cleanup_failures` - Cleanup failures to append before canonical
    ///   sorting.
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
    ///
    /// # Parameters
    ///
    /// * `output` - Output observed before the error was finalized.
    #[inline]
    pub(crate) fn with_output(mut self, output: CommandOutput) -> Self {
        self.output = Some(Box::new(output));
        self
    }

    /// Converts a helper error into its cleanup representation.
    pub(crate) fn into_cleanup_failure(self) -> Option<CommandCleanupFailure> {
        primary_cleanup_failure(*self.reason)
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
        primary_cleanup_failure(*reason)
            .into_iter()
            .chain(cleanup_failures)
            .collect()
    }

    /// Merges a finalization error without changing the selected primary
    /// reason.
    ///
    /// Preserves this error's output first, then moves either output carrier
    /// from the finalization error when neither is present. Records helper/time
    /// failures as cleanup evidence. The input must be a finalization error.
    ///
    /// # Parameters
    ///
    /// * `error` - Error produced during finalization of this command.
    pub(crate) fn with_finalization_error(mut self, mut error: CommandError) -> Self {
        if self.output.is_none() && self.captured_output.is_none() {
            self.output = error.output.take();
            self.captured_output = error.captured_output.take();
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

    /// Returns retained output with an exit status and elapsed time.
    ///
    /// # Returns
    ///
    /// `Some(output)` when a `CommandOutput` was retained. Its streams may be
    /// incomplete or truncated. `None` means this carrier is absent, not that
    /// no bytes were captured. For example, an output-budget failure can retain
    /// bytes through [`Self::captured_output`] without a `CommandOutput`.
    ///
    /// # Examples
    ///
    /// ```
    /// use qubit_command::CommandError;
    ///
    /// fn inspect(error: &CommandError) {
    ///     if let Some(output) = error.output() {
    ///         let _ = (output.exit_code(), output.stdout(), output.stderr());
    ///     } else if let Some(captured) = error.captured_output() {
    ///         let _ = (captured.stdout(), captured.stderr());
    ///         let _ = (captured.stdout_complete(), captured.stderr_complete());
    ///     }
    /// }
    /// ```
    #[must_use]
    #[inline(always)]
    pub fn output(&self) -> Option<&CommandOutput> {
        self.output.as_deref()
    }

    /// Returns captured bytes, including partial output when no real process
    /// status exists.
    #[must_use]
    #[inline(always)]
    pub fn captured_output(&self) -> Option<&CommandCapturedOutput> {
        self.output
            .as_deref()
            .map(CommandOutput::captured_output)
            .or(self.captured_output.as_deref())
    }

    /// Attaches captured bytes when no completed output carrier is available.
    ///
    /// # Parameters
    ///
    /// * `output` - Captured stream bytes and their completeness metadata.
    pub(crate) fn with_captured_output(mut self, output: CommandCapturedOutput) -> Self {
        self.captured_output = Some(Box::new(output));
        self
    }

    /// Takes the retained stream bytes without consuming this error.
    ///
    /// A retained [`CommandOutput`] takes precedence over the independent
    /// captured-byte carrier, matching [`Self::into_captured_output`]. Any
    /// unused carrier is discarded. The primary reason and cleanup failures
    /// remain available for subsequent inspection or consumption.
    ///
    /// # Returns
    ///
    /// `Some(captured)` when either output carrier was retained, including an
    /// empty capture. The stream buffers are moved without cloning. `None`
    /// means neither carrier was retained.
    pub(crate) fn take_captured_output(&mut self) -> Option<CommandCapturedOutput> {
        let output = self.output.take();
        let captured = self.captured_output.take();
        match output {
            Some(output) => Some((*output).into_captured_output()),
            None => captured.map(|captured| *captured),
        }
    }

    /// Consumes the error and returns its retained `CommandOutput`.
    ///
    /// # Returns
    ///
    /// `Some(output)` when output with an exit status and elapsed time was
    /// retained. `None` does not mean no bytes were captured. Consuming this
    /// error discards any independent captured-byte carrier. Inspect
    /// [`Self::captured_output`] first, and clone that value if owned bytes
    /// are needed after consuming the error. Use
    /// [`Self::into_captured_output`] to move the captured bytes out instead.
    #[must_use]
    #[inline(always)]
    pub fn into_output(self) -> Option<CommandOutput> {
        self.output.map(|output| *output)
    }

    /// Consumes the error and returns its retained captured bytes.
    ///
    /// # Returns
    ///
    /// `Some(captured)` when either output carrier was retained, including an
    /// empty capture. A retained [`CommandOutput`] takes precedence over the
    /// independent captured-byte carrier. The returned value preserves stream
    /// completeness and truncation markers and moves the retained byte buffers
    /// without cloning them. `None` means neither carrier was retained.
    ///
    /// Consuming the error also consumes its diagnostic information. Inspect
    /// the error's reason, cleanup failures, or other diagnostics before
    /// calling this method when they are needed afterward.
    ///
    /// # Examples
    ///
    /// ```
    /// use qubit_command::{CommandCapturedOutput, CommandError};
    ///
    /// fn retain_capture(error: CommandError) -> Option<CommandCapturedOutput> {
    ///     // Inspect diagnostics before consuming the error if needed.
    ///     error.into_captured_output()
    /// }
    /// ```
    #[must_use]
    #[inline]
    pub fn into_captured_output(self) -> Option<CommandCapturedOutput> {
        let mut this = self;
        this.take_captured_output()
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

/// Converts a primary helper failure into cleanup evidence.
///
/// # Parameters
///
/// * `reason` - Owned primary reason whose source and path are moved.
///
/// # Returns
///
/// `Some(failure)` for time or stream I/O failures; `None` for other reasons.
fn primary_cleanup_failure(reason: CommandErrorReason) -> Option<CommandCleanupFailure> {
    match reason {
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

/// Returns the canonical cleanup resource rank.
///
/// # Parameters
///
/// * `failure` - Cleanup failure to place in canonical order.
///
/// # Returns
///
/// The numeric rank for the failure's resource category.
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
    /// Formats the primary failure and the count of cleanup failures.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Formatter receiving the user-facing error message.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let command = &self.command;
        match (self.reason.as_ref(), self.output.as_deref()) {
            (CommandErrorReason::SpawnFailed { source }, _) => {
                write!(
                    formatter,
                    "failed to spawn command `{command}`: {}",
                    io_source_summary(source)
                )
            }
            (CommandErrorReason::WaitFailed { source }, _) => {
                write!(
                    formatter,
                    "failed to wait for command `{command}`: {}",
                    io_source_summary(source)
                )
            }
            (CommandErrorReason::CancelledBeforeStart, _) => {
                write!(formatter, "command `{command}` was cancelled before it started")
            }
            (CommandErrorReason::ReadOutputFailed { stream, source }, _) => {
                write!(
                    formatter,
                    "failed to read {stream} for command `{command}`: {}",
                    io_source_summary(source)
                )
            }
            (CommandErrorReason::OpenInputFailed { source, .. }, _) => write!(
                formatter,
                "failed to open stdin file `<redacted path>` for command `{command}`: {}",
                io_source_summary(source)
            ),
            (CommandErrorReason::NonRegularInputFile { .. }, _) => write!(
                formatter,
                "stdin path `<redacted path>` for command `{command}` is not an ordinary file"
            ),
            (CommandErrorReason::OpenOutputFailed { stream, source, .. }, _) => write!(
                formatter,
                "failed to open {stream} file `<redacted path>` for command `{command}`: {}",
                io_source_summary(source)
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
                    "failed to inspect I/O file '<redacted path>' for command '{command}': {}",
                    io_source_summary(source)
                )
            }
            (CommandErrorReason::StartInputThreadFailed { source }, _) => {
                write!(
                    formatter,
                    "failed to start stdin writer for command '{command}': {}",
                    io_source_summary(source)
                )
            }
            (CommandErrorReason::StartOutputThreadFailed { stream, source }, _) => write!(
                formatter,
                "failed to start {stream} reader for command '{command}': {}",
                io_source_summary(source)
            ),
            (CommandErrorReason::TimeFailed { .. }, _) => {
                write!(
                    formatter,
                    "time handling failed for command '{command}': {TIME_SOURCE_SUMMARY}"
                )
            }
            (CommandErrorReason::WriteInputFailed { source }, _) => {
                write!(
                    formatter,
                    "failed to write stdin for command `{command}`: {}",
                    io_source_summary(source)
                )
            }
            (CommandErrorReason::WriteOutputFailed { stream, source, .. }, _) => write!(
                formatter,
                "failed to write {stream} for command `{command}` to `<redacted path>`: {}",
                io_source_summary(source)
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
            (CommandErrorReason::OutputLimitExceeded { limit, observed }, _) => write!(
                formatter,
                "command `{command}` exceeded its combined output limit of {limit} bytes (observed {observed})"
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
    /// Returns the underlying operating-system or timer error, when present.
    ///
    /// # Parameters
    ///
    /// * `self` - Error whose primary reason may contain a source error.
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
    /// Converts a detailed reason into its stable category.
    ///
    /// # Parameters
    ///
    /// * `reason` - Detailed failure reason to classify.
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
            CommandErrorReason::OutputLimitExceeded { .. } => Self::OutputLimitExceeded,
            CommandErrorReason::UnexpectedExit { .. } => Self::UnexpectedExit,
        }
    }
}

/// Formats the observed termination detail for an unexpected command exit.
///
/// # Parameters
///
/// * `exit_code` - Numeric exit code, if the process reported one.
/// * `output` - Completed output metadata used to inspect Unix termination
///   signals.
///
/// # Returns
///
/// A short signal or exit-code description for the display message.
fn unexpected_exit_detail(exit_code: &Option<i32>, output: Option<&CommandOutput>) -> String {
    #[cfg(unix)]
    if let Some(signal) = output.and_then(CommandOutput::termination_signal) {
        return format!("signal {signal}");
    }
    #[cfg(not(unix))]
    let _ = output;
    format!("code {exit_code:?}")
}
