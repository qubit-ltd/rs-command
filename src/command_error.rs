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
    /// Operation-manager stop cause associated with this command error.
    stop_reason: Option<qubit_operation::StopReason>,
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
            stop_reason: None,
            cleanup_failures: Vec::new(),
        }
    }

    /// Reclassifies an owned-file validation error as managed preparation.
    ///
    /// # Parameters
    /// * `stage`: Preparation phase assigned to the validation error.
    pub(crate) fn into_file_preparation_failure(self, stage: crate::FilePreparationStage) -> Self {
        let Self {
            command,
            reason,
            output,
            captured_output,
            stop_reason,
            cleanup_failures,
        } = self;
        let reason = match *reason {
            CommandErrorReason::OpenInputFailed { source, .. } => CommandErrorReason::FilePreparationFailed {
                stage,
                direction: Some(crate::FilePreparationDirection::Stdin),
                source,
            },
            CommandErrorReason::NonRegularInputFile { .. } => CommandErrorReason::FilePreparationFailed {
                stage,
                direction: Some(crate::FilePreparationDirection::Stdin),
                source: io::Error::new(io::ErrorKind::InvalidInput, "input is not a regular file"),
            },
            CommandErrorReason::OpenOutputFailed { stream, source, .. } => {
                let direction = Some(match stream {
                    OutputStream::Stdout => crate::FilePreparationDirection::Stdout,
                    OutputStream::Stderr => crate::FilePreparationDirection::Stderr,
                });
                CommandErrorReason::FilePreparationFailed {
                    stage,
                    direction,
                    source,
                }
            }
            CommandErrorReason::NonRegularOutputFile { stream, .. } => {
                let direction = Some(match stream {
                    OutputStream::Stdout => crate::FilePreparationDirection::Stdout,
                    OutputStream::Stderr => crate::FilePreparationDirection::Stderr,
                });
                CommandErrorReason::FilePreparationFailed {
                    stage,
                    direction,
                    source: io::Error::new(io::ErrorKind::InvalidInput, "output is not a regular file"),
                }
            }
            CommandErrorReason::WriteOutputFailed { stream, source, .. } => {
                let direction = Some(match stream {
                    OutputStream::Stdout => crate::FilePreparationDirection::Stdout,
                    OutputStream::Stderr => crate::FilePreparationDirection::Stderr,
                });
                CommandErrorReason::FilePreparationFailed {
                    stage,
                    direction,
                    source,
                }
            }
            other => other,
        };
        Self {
            command,
            reason: Box::new(reason),
            output,
            captured_output,
            stop_reason,
            cleanup_failures,
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

    /// Adds the operation-manager stop cause while retaining captured output
    /// and cleanup diagnostics.
    ///
    /// # Parameters
    /// * `reason`: The operation-manager stop cause.
    pub(crate) fn with_managed_stop_reason(mut self, reason: qubit_operation::StopReason) -> Self {
        self.stop_reason = Some(reason);
        if reason != qubit_operation::StopReason::UserCancelled {
            self.reason = Box::new(CommandErrorReason::Stopped { reason });
        }
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

    /// Returns the command representation rendered with the configured
    /// diagnostic redaction policy and explicit sensitivity markers.
    ///
    /// Dedicated working-directory, stdin, and tee path settings are omitted
    /// or masked in diagnostics. This does not guarantee that executable paths
    /// or paths passed as ordinary arguments are hidden. Mark sensitive path
    /// arguments with [`crate::Command::sensitive_arg`] or
    /// [`crate::Command::sensitive_arg_os`].
    ///
    /// # Returns
    ///
    /// The stored diagnostic command text. Ordinary argument values may remain
    /// visible unless explicit sensitivity markers or redaction rules hide
    /// them.
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

    /// Returns the resource manager's classification when registration failed.
    ///
    /// # Returns
    ///
    /// The original manager category for a resource registration failure, or
    /// `None` when this error did not originate from resource registration.
    #[must_use]
    pub fn resource_registration_error_kind(&self) -> Option<qubit_operation::ResourceManageErrorKind> {
        let source = match self.reason.as_ref() {
            CommandErrorReason::FilePreparationFailed { source, .. } => source,
            _ => return None,
        };
        source
            .get_ref()
            .and_then(|source| {
                source.downcast_ref::<crate::execution::files::registration_category::RegistrationCategory>()
            })
            .map(|category| category.0)
    }

    /// Returns the operation-manager stop cause associated with this error.
    ///
    /// # Returns
    /// The accepted stop cause when this error came from a stopped managed
    /// operation.
    #[must_use]
    #[inline]
    pub fn stop_reason(&self) -> Option<qubit_operation::StopReason> {
        self.stop_reason
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

/// Converts a managed file preparation failure into its public error carrier.
///
/// # Parameters
/// * `stage` - File preparation phase that failed.
/// * `direction` - Affected stream, when known.
/// * `source` - Original preparation error and optional typed manager cause.
pub(crate) fn managed_file_preparation_error(
    stage: crate::FilePreparationStage,
    direction: Option<crate::FilePreparationDirection>,
    source: io::Error,
) -> CommandError {
    CommandError::from_reason(
        "<managed command>",
        CommandErrorReason::FilePreparationFailed {
            stage,
            direction,
            source,
        },
        None,
    )
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
            (CommandErrorReason::Stopped { reason }, _) => {
                write!(formatter, "managed command operation stopped: {reason:?}")
            }
            (CommandErrorReason::ManagedOperationFailed { diagnostic }, _) => {
                write!(formatter, "managed command operation failed: {diagnostic}")
            }
            (CommandErrorReason::FilePreparationFailed { stage, direction, .. }, _) => write!(
                formatter,
                "managed file preparation failed during {stage:?}{} for command `{command}`",
                direction.map(|value| format!(" ({value:?})")).unwrap_or_default()
            ),
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
            | CommandErrorReason::WriteOutputFailed { source, .. }
            | CommandErrorReason::FilePreparationFailed { source, .. } => Some(source),
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
            CommandErrorReason::Stopped { .. } => Self::Stopped,
            CommandErrorReason::OutputTruncated => Self::OutputTruncated,
            CommandErrorReason::OutputLimitExceeded { .. } => Self::OutputLimitExceeded,
            CommandErrorReason::UnexpectedExit { .. } => Self::UnexpectedExit,
            CommandErrorReason::ManagedOperationFailed { .. } => Self::ManagedOperationFailed,
            CommandErrorReason::FilePreparationFailed { .. } => Self::FilePreparationFailed,
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::process::ExitStatus;
    use std::time::Duration;

    use qubit_clock::TimeError;

    use super::CommandCapturedOutput;
    use super::CommandCleanupFailure;
    use super::CommandError;
    use super::CommandErrorKind;
    use super::CommandErrorReason;
    use super::CommandOutput;
    use super::OutputStream;

    #[test]
    fn test_resource_registration_category_accessor_reads_typed_source() {
        let error = CommandError::from_reason(
            "managed command",
            CommandErrorReason::FilePreparationFailed {
                stage: crate::FilePreparationStage::Validation,
                direction: Some(crate::FilePreparationDirection::Stdin),
                source: io::Error::other(crate::execution::files::registration_category::RegistrationCategory(
                    qubit_operation::ResourceManageErrorKind::Stopped,
                )),
            },
            None,
        );
        assert_eq!(
            error.resource_registration_error_kind(),
            Some(qubit_operation::ResourceManageErrorKind::Stopped)
        );
        let unrelated = CommandError::from_reason("command", CommandErrorReason::CancelledBeforeStart, None);
        assert_eq!(unrelated.resource_registration_error_kind(), None);
    }

    #[test]
    fn test_file_preparation_mapping_preserves_source_and_stream_direction() {
        for (stream, direction, path) in [
            (
                OutputStream::Stdout,
                crate::FilePreparationDirection::Stdout,
                "stdout.log",
            ),
            (
                OutputStream::Stderr,
                crate::FilePreparationDirection::Stderr,
                "stderr.log",
            ),
        ] {
            let open_error = CommandError::from_reason(
                "prepared command",
                CommandErrorReason::OpenOutputFailed {
                    stream,
                    path: path.into(),
                    source: io::Error::new(io::ErrorKind::NotFound, "fixture open failure"),
                },
                None,
            )
            .into_file_preparation_failure(crate::FilePreparationStage::Validation);
            assert!(matches!(
                open_error.reason(),
                CommandErrorReason::FilePreparationFailed {
                    stage: crate::FilePreparationStage::Validation,
                    direction: Some(actual),
                    source,
                } if *actual == direction && source.kind() == io::ErrorKind::NotFound
            ));

            let error = CommandError::from_reason(
                "prepared command",
                CommandErrorReason::WriteOutputFailed {
                    stream,
                    path: path.into(),
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "fixture write denied"),
                },
                None,
            )
            .into_file_preparation_failure(crate::FilePreparationStage::Validation);

            assert_eq!(error.kind(), CommandErrorKind::FilePreparationFailed);
            assert!(matches!(
                error.reason(),
                CommandErrorReason::FilePreparationFailed {
                    stage: crate::FilePreparationStage::Validation,
                    direction: Some(actual_direction),
                    source,
                } if *actual_direction == direction
                    && source.kind() == io::ErrorKind::PermissionDenied
                    && source.to_string() == "fixture write denied"
            ));
        }
    }

    #[test]
    fn test_resource_stdin_path_failures_map_to_validation_stdin() {
        let open_error = CommandError::from_reason(
            "prepared command",
            CommandErrorReason::OpenInputFailed {
                path: "missing-input".into(),
                source: io::Error::new(io::ErrorKind::NotFound, "fixture input failure"),
            },
            None,
        )
        .into_file_preparation_failure(crate::FilePreparationStage::Validation);
        assert!(matches!(
            open_error.reason(),
            CommandErrorReason::FilePreparationFailed {
                stage: crate::FilePreparationStage::Validation,
                direction: Some(crate::FilePreparationDirection::Stdin),
                source,
            } if source.kind() == io::ErrorKind::NotFound
                && source.to_string() == "fixture input failure"
        ));

        let type_error = CommandError::from_reason(
            "prepared command",
            CommandErrorReason::NonRegularInputFile {
                path: "directory-input".into(),
            },
            None,
        )
        .into_file_preparation_failure(crate::FilePreparationStage::Validation);
        assert!(matches!(
            type_error.reason(),
            CommandErrorReason::FilePreparationFailed {
                stage: crate::FilePreparationStage::Validation,
                direction: Some(crate::FilePreparationDirection::Stdin),
                source,
            } if source.kind() == io::ErrorKind::InvalidInput
        ));
    }

    fn status(code: i32) -> ExitStatus {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(code << 8)
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(u32::try_from(code).expect("test exit code must be nonnegative"))
        }
    }

    fn output(code: i32, stdout: &[u8], stderr: &[u8]) -> CommandOutput {
        CommandOutput::new(
            status(code),
            (stdout.to_vec(), false, true),
            (stderr.to_vec(), false, true),
            Duration::from_millis(17),
        )
    }

    fn cleanup_labels(failures: &[CommandCleanupFailure]) -> Vec<&'static str> {
        failures
            .iter()
            .map(|failure| match failure {
                CommandCleanupFailure::ProcessTreeTermination { .. } => "tree",
                CommandCleanupFailure::ChildTermination { .. } => "child",
                CommandCleanupFailure::Wait { .. } => "wait",
                CommandCleanupFailure::Time { .. } => "time",
                CommandCleanupFailure::StdoutCancellation { .. } => "stdout-cancel",
                CommandCleanupFailure::StdoutRead { .. } => "stdout-read",
                CommandCleanupFailure::StdoutWrite { .. } => "stdout-write",
                CommandCleanupFailure::StderrCancellation { .. } => "stderr-cancel",
                CommandCleanupFailure::StderrRead { .. } => "stderr-read",
                CommandCleanupFailure::StderrWrite { .. } => "stderr-write",
                CommandCleanupFailure::Stdin { .. } => "stdin",
                CommandCleanupFailure::StdinCancellation { .. } => "stdin-cancel",
            })
            .collect()
    }

    #[test]
    fn test_command_accessors_and_output_carriers_preserve_their_contract() {
        let completed = output(7, b"completed", b"stderr");
        let captured = CommandCapturedOutput::new(
            (b"fallback".to_vec(), true, false),
            (b"fallback err".to_vec(), false, true),
        );
        let error = CommandError::from_reason(
            String::from("redacted command"),
            CommandErrorReason::Cancelled,
            Some(Box::new(completed)),
        )
        .with_captured_output(captured);

        assert_eq!(error.command(), "redacted command");
        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        assert!(matches!(error.reason(), CommandErrorReason::Cancelled));
        assert!(!error.is_unexpected_exit());
        assert_eq!(error.exit_code(), Some(7));
        assert_eq!(error.output().unwrap().stdout(), b"completed");
        assert_eq!(error.output().unwrap().elapsed(), Duration::from_millis(17));
        let capture = error.captured_output().unwrap();
        assert_eq!(capture.stdout(), b"completed");
        assert_eq!(capture.stderr(), b"stderr");
        assert!(!capture.stdout_truncated());

        let owned_output = error.into_output().expect("completed output is retained");
        assert_eq!(owned_output.stdout(), b"completed");
    }

    #[test]
    fn test_captured_output_carrier_is_used_when_no_completed_output_exists() {
        let error = CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_captured_output(
            CommandCapturedOutput::new((b"partial".to_vec(), true, false), (b"tail".to_vec(), false, true)),
        );
        assert!(error.output().is_none());
        assert_eq!(error.exit_code(), None);
        let capture = error.captured_output().unwrap();
        assert_eq!(capture.stdout(), b"partial");
        assert_eq!(capture.stderr(), b"tail");
        assert!(capture.stdout_truncated());
        assert!(!capture.stdout_complete());

        let moved = error.into_captured_output().expect("partial bytes are retained");
        assert_eq!(moved.stdout(), b"partial");
        assert_eq!(moved.stderr(), b"tail");
    }

    #[test]
    fn test_unexpected_exit_uses_reason_code_and_completed_output_for_display() {
        let expected = vec![0, 2];
        let error = CommandError::from_reason(
            "command",
            CommandErrorReason::UnexpectedExit {
                exit_code: Some(9),
                expected: expected.clone(),
            },
            Some(Box::new(output(4, b"out", b"err"))),
        );
        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert!(error.is_unexpected_exit());
        assert_eq!(error.exit_code(), Some(9));
        assert!(error.to_string().contains("code Some(9)"));
        assert!(error.to_string().contains("expected one of [0, 2]"));

        let no_code = CommandError::from_reason(
            "command",
            CommandErrorReason::UnexpectedExit {
                exit_code: None,
                expected,
            },
            None,
        );
        assert_eq!(no_code.exit_code(), None);
        assert!(no_code.to_string().contains("code None"));
    }

    #[test]
    fn test_cleanup_failures_sort_by_resource_and_source_accessors_find_first() {
        let error = CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_cleanup_failures([
            CommandCleanupFailure::StdinCancellation {
                source: io::Error::other("stdin"),
            },
            CommandCleanupFailure::StderrWrite {
                path: "stderr.log".into(),
                source: io::Error::other("stderr write"),
            },
            CommandCleanupFailure::StdoutCancellation {
                source: io::Error::other("stdout cancel"),
            },
            CommandCleanupFailure::Time {
                source: TimeError::InstantOverflow,
            },
            CommandCleanupFailure::Wait {
                source: io::Error::other("wait"),
            },
            CommandCleanupFailure::ChildTermination {
                source: io::Error::other("child first"),
            },
            CommandCleanupFailure::ProcessTreeTermination {
                source: io::Error::other("tree first"),
            },
            CommandCleanupFailure::ChildTermination {
                source: io::Error::other("child second"),
            },
            CommandCleanupFailure::StdoutRead {
                source: io::Error::other("stdout read"),
            },
            CommandCleanupFailure::StderrCancellation {
                source: io::Error::other("stderr cancel"),
            },
        ]);

        assert_eq!(
            cleanup_labels(error.cleanup_failures()),
            [
                "tree",
                "child",
                "child",
                "wait",
                "time",
                "stdout-cancel",
                "stdout-read",
                "stderr-write",
                "stderr-cancel",
                "stdin-cancel"
            ]
        );
        assert_eq!(error.process_tree_source().unwrap().to_string(), "tree first");
        assert_eq!(error.child_source().unwrap().to_string(), "child first");
        let empty = CommandError::from_reason("command", CommandErrorReason::Cancelled, None);
        assert!(empty.process_tree_source().is_none());
        assert!(empty.child_source().is_none());
    }

    #[test]
    fn test_helper_primary_reasons_convert_to_the_matching_cleanup_variant() {
        let cases = [
            (
                CommandErrorReason::TimeFailed {
                    source: TimeError::InstantOverflow,
                },
                "time",
            ),
            (
                CommandErrorReason::WriteInputFailed {
                    source: io::Error::other("stdin"),
                },
                "stdin",
            ),
            (
                CommandErrorReason::ReadOutputFailed {
                    stream: OutputStream::Stdout,
                    source: io::Error::other("stdout read"),
                },
                "stdout-read",
            ),
            (
                CommandErrorReason::ReadOutputFailed {
                    stream: OutputStream::Stderr,
                    source: io::Error::other("stderr read"),
                },
                "stderr-read",
            ),
            (
                CommandErrorReason::WriteOutputFailed {
                    stream: OutputStream::Stdout,
                    path: "out.log".into(),
                    source: io::Error::other("stdout write"),
                },
                "stdout-write",
            ),
            (
                CommandErrorReason::WriteOutputFailed {
                    stream: OutputStream::Stderr,
                    path: "err.log".into(),
                    source: io::Error::other("stderr write"),
                },
                "stderr-write",
            ),
        ];

        for (reason, expected) in cases {
            let failure = CommandError::from_reason("command", reason, None)
                .into_cleanup_failure()
                .expect("helper failure maps to cleanup evidence");
            assert_eq!(cleanup_labels(&[failure]), [expected]);
        }

        let ordinary = CommandError::from_reason("command", CommandErrorReason::Cancelled, None);
        assert!(ordinary.into_cleanup_failure().is_none());
    }

    #[test]
    fn test_consuming_error_cleanup_includes_primary_helper_before_sorted_secondary_failures() {
        let error = CommandError::from_reason(
            "command",
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stderr,
                source: io::Error::other("primary read"),
            },
            None,
        )
        .with_cleanup_failures([
            CommandCleanupFailure::Stdin {
                source: io::Error::other("cleanup stdin"),
            },
            CommandCleanupFailure::StdoutRead {
                source: io::Error::other("cleanup stdout"),
            },
        ]);
        let failures = error.into_cleanup_failures();
        assert_eq!(cleanup_labels(&failures), ["stderr-read", "stdout-read", "stdin"]);
        match &failures[0] {
            CommandCleanupFailure::StderrRead { source } => {
                assert_eq!(source.to_string(), "primary read")
            }
            _ => panic!("primary failure should be converted first"),
        }

        let no_helper = CommandError::from_reason("command", CommandErrorReason::Cancelled, None)
            .with_cleanup_failures([CommandCleanupFailure::Time {
                source: TimeError::InstantOverflow,
            }])
            .into_cleanup_failures();
        assert_eq!(cleanup_labels(&no_helper), ["time"]);
    }

    #[test]
    fn test_finalization_keeps_primary_and_output_while_appending_cleanup_in_order() {
        let primary_capture = CommandCapturedOutput::new((b"primary".to_vec(), false, true), (Vec::new(), false, true));
        let primary = CommandError::from_reason("<redacted command>", CommandErrorReason::Cancelled, None)
            .with_captured_output(primary_capture);
        let finalization = CommandError::from_reason(
            "<redacted command>",
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stdout,
                source: io::Error::other("final read"),
            },
            Some(Box::new(output(5, b"final", b"err"))),
        )
        .with_cleanup_failures([CommandCleanupFailure::Stdin {
            source: io::Error::other("stdin cleanup"),
        }]);

        let mut error = primary.with_finalization_error(finalization);
        assert!(matches!(error.reason(), CommandErrorReason::Cancelled));
        assert_eq!(error.captured_output().unwrap().stdout(), b"primary");
        assert_eq!(cleanup_labels(error.cleanup_failures()), ["stdout-read", "stdin"]);
        match &error.cleanup_failures()[0] {
            CommandCleanupFailure::StdoutRead { source } => {
                assert_eq!(source.to_string(), "final read")
            }
            _ => panic!("finalization read error should remain available as cleanup evidence"),
        }
        let moved = error.take_captured_output().unwrap();
        assert_eq!(moved.stdout(), b"primary");
        assert!(error.take_captured_output().is_none());
    }
}
