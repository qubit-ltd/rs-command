// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Detailed primary failure reasons for command execution.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use qubit_clock::TimeError;
use qubit_redact::Redactor;

use crate::OutputStream;

/// Redacts one debug-only value before it crosses the diagnostic boundary.
fn redacted_debug_text(value: &impl fmt::Debug) -> String {
    Redactor::strict()
        .redact_field("command_path", &format_args!("{value:?}"))
        .into_text_or_marker("<redaction incomplete>")
        .into_string()
}

/// Detailed primary reason carried by [`crate::CommandError`].
///
/// # Examples
///
/// ```
/// use qubit_command::{Command, CommandErrorReason, CommandRunner};
///
/// let error = CommandRunner::without_timeout()
///     .run(Command::new("__qubit_command_example_missing_executable__"))
///     .expect_err("the example executable should not exist");
/// assert!(matches!(error.reason(), CommandErrorReason::SpawnFailed { .. }));
/// ```
#[non_exhaustive]
#[must_use]
pub enum CommandErrorReason {
    /// The process could not be spawned.
    SpawnFailed {
        /// Operating-system spawn error.
        source: io::Error,
    },
    /// Waiting for process completion failed.
    WaitFailed {
        /// Operating-system wait error.
        source: io::Error,
    },
    /// Cancellation was requested before startup.
    CancelledBeforeStart,
    /// Termination after a timeout failed.
    KillFailed {
        /// Timeout that was exceeded.
        timeout: Duration,
        /// Process-tree termination error.
        process_tree_source: io::Error,
        /// Direct-child termination error.
        child_source: io::Error,
    },
    /// Reading a captured stream failed.
    ReadOutputFailed {
        /// Output stream whose reader failed.
        stream: OutputStream,
        /// Operating-system read error.
        source: io::Error,
    },
    /// Opening stdin failed.
    OpenInputFailed {
        /// Configured stdin path.
        path: PathBuf,
        /// Operating-system open error.
        source: io::Error,
    },
    /// Stdin path is not a regular file.
    NonRegularInputFile {
        /// Configured stdin path.
        path: PathBuf,
    },
    /// Opening an output tee failed.
    OpenOutputFailed {
        /// Output stream whose tee could not be opened.
        stream: OutputStream,
        /// Configured output path.
        path: PathBuf,
        /// Operating-system open error.
        source: io::Error,
    },
    /// Output tee path is not a regular file.
    NonRegularOutputFile {
        /// Output stream receiving the tee.
        stream: OutputStream,
        /// Configured output path.
        path: PathBuf,
    },
    /// Input and output files conflict.
    InputOutputConflict {
        /// Configured stdin path.
        input_path: PathBuf,
        /// Output stream whose path conflicts with stdin.
        output_stream: OutputStream,
        /// Conflicting output path.
        output_path: PathBuf,
    },
    /// Output tee files conflict.
    OutputFilesConflict {
        /// Configured stdout tee path.
        stdout_path: PathBuf,
        /// Configured stderr tee path.
        stderr_path: PathBuf,
    },
    /// An I/O file could not be inspected.
    InspectIoFileFailed {
        /// Configured path that could not be inspected.
        path: PathBuf,
        /// Operating-system inspection error.
        source: io::Error,
    },
    /// The stdin helper could not be started.
    StartInputThreadFailed {
        /// Thread creation error.
        source: io::Error,
    },
    /// An output helper could not be started.
    StartOutputThreadFailed {
        /// Output stream whose helper could not start.
        stream: OutputStream,
        /// Thread creation error.
        source: io::Error,
    },
    /// Clock or timer handling failed.
    TimeFailed {
        /// Clock or timer error.
        source: TimeError,
    },
    /// Writing configured stdin failed.
    WriteInputFailed {
        /// Operating-system write error.
        source: io::Error,
    },
    /// Writing a captured stream to a tee failed.
    WriteOutputFailed {
        /// Output stream whose tee write failed.
        stream: OutputStream,
        /// Configured output path.
        path: PathBuf,
        /// Operating-system write error.
        source: io::Error,
    },
    /// The command exceeded its timeout.
    TimedOut {
        /// Timeout that was exceeded.
        timeout: Duration,
    },
    /// The command was cancelled after startup.
    Cancelled,
    /// Process-tree cancellation failed.
    CancelFailed {
        /// Process-tree termination error.
        process_tree_source: io::Error,
        /// Direct-child termination error.
        child_source: io::Error,
    },
    /// Successful command output was truncated.
    OutputTruncated,
    /// The process exited with an unconfigured status.
    UnexpectedExit {
        /// Exit code reported by the process, when available.
        exit_code: Option<i32>,
        /// Exit codes configured as successful.
        expected: Vec<i32>,
    },
}

impl fmt::Debug for CommandErrorReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpawnFailed { source } => formatter.debug_struct("SpawnFailed").field("source", source).finish(),
            Self::WaitFailed { source } => formatter.debug_struct("WaitFailed").field("source", source).finish(),
            Self::CancelledBeforeStart => formatter.write_str("CancelledBeforeStart"),
            Self::KillFailed {
                timeout,
                process_tree_source,
                child_source,
            } => formatter
                .debug_struct("KillFailed")
                .field("timeout", timeout)
                .field("process_tree_source", process_tree_source)
                .field("child_source", child_source)
                .finish(),
            Self::ReadOutputFailed { stream, source } => formatter
                .debug_struct("ReadOutputFailed")
                .field("stream", stream)
                .field("source", source)
                .finish(),
            Self::OpenInputFailed { path, source } => formatter
                .debug_struct("OpenInputFailed")
                .field("path", &redacted_debug_text(path))
                .field("source", source)
                .finish(),
            Self::NonRegularInputFile { path } => formatter
                .debug_struct("NonRegularInputFile")
                .field("path", &redacted_debug_text(path))
                .finish(),
            Self::OpenOutputFailed { stream, path, source } => formatter
                .debug_struct("OpenOutputFailed")
                .field("stream", stream)
                .field("path", &redacted_debug_text(path))
                .field("source", source)
                .finish(),
            Self::NonRegularOutputFile { stream, path } => formatter
                .debug_struct("NonRegularOutputFile")
                .field("stream", stream)
                .field("path", &redacted_debug_text(path))
                .finish(),
            Self::InputOutputConflict {
                input_path,
                output_stream,
                output_path,
            } => formatter
                .debug_struct("InputOutputConflict")
                .field("input_path", &redacted_debug_text(input_path))
                .field("output_stream", output_stream)
                .field("output_path", &redacted_debug_text(output_path))
                .finish(),
            Self::OutputFilesConflict {
                stdout_path,
                stderr_path,
            } => formatter
                .debug_struct("OutputFilesConflict")
                .field("stdout_path", &redacted_debug_text(stdout_path))
                .field("stderr_path", &redacted_debug_text(stderr_path))
                .finish(),
            Self::InspectIoFileFailed { path, source } => formatter
                .debug_struct("InspectIoFileFailed")
                .field("path", &redacted_debug_text(path))
                .field("source", source)
                .finish(),
            Self::StartInputThreadFailed { source } => formatter
                .debug_struct("StartInputThreadFailed")
                .field("source", source)
                .finish(),
            Self::StartOutputThreadFailed { stream, source } => formatter
                .debug_struct("StartOutputThreadFailed")
                .field("stream", stream)
                .field("source", source)
                .finish(),
            Self::TimeFailed { source } => formatter.debug_struct("TimeFailed").field("source", source).finish(),
            Self::WriteInputFailed { source } => formatter
                .debug_struct("WriteInputFailed")
                .field("source", source)
                .finish(),
            Self::WriteOutputFailed { stream, path, source } => formatter
                .debug_struct("WriteOutputFailed")
                .field("stream", stream)
                .field("path", &redacted_debug_text(path))
                .field("source", source)
                .finish(),
            Self::TimedOut { timeout } => formatter.debug_struct("TimedOut").field("timeout", timeout).finish(),
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::CancelFailed {
                process_tree_source,
                child_source,
            } => formatter
                .debug_struct("CancelFailed")
                .field("process_tree_source", process_tree_source)
                .field("child_source", child_source)
                .finish(),
            Self::OutputTruncated => formatter.write_str("OutputTruncated"),
            Self::UnexpectedExit { exit_code, expected } => formatter
                .debug_struct("UnexpectedExit")
                .field("exit_code", exit_code)
                .field("expected", expected)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;
    use std::time::Duration;

    use qubit_clock::TimeError;
    use qubit_clock::TimerUnavailableError;

    use super::CommandErrorReason;
    use crate::CommandCleanupFailure;
    use crate::CommandError;
    use crate::OutputStream;

    fn io_error() -> io::Error {
        io::Error::other("injected error source")
    }

    #[test]
    fn test_command_error_formats_every_reason_variant() {
        let reasons = vec![
            CommandErrorReason::SpawnFailed { source: io_error() },
            CommandErrorReason::WaitFailed { source: io_error() },
            CommandErrorReason::CancelledBeforeStart,
            CommandErrorReason::KillFailed {
                timeout: Duration::from_secs(1),
                process_tree_source: io_error(),
                child_source: io_error(),
            },
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stdout,
                source: io_error(),
            },
            CommandErrorReason::OpenInputFailed {
                path: "input".into(),
                source: io_error(),
            },
            CommandErrorReason::NonRegularInputFile {
                path: "input".into(),
            },
            CommandErrorReason::OpenOutputFailed {
                stream: OutputStream::Stdout,
                path: "output".into(),
                source: io_error(),
            },
            CommandErrorReason::NonRegularOutputFile {
                stream: OutputStream::Stderr,
                path: "output".into(),
            },
            CommandErrorReason::InputOutputConflict {
                input_path: "input".into(),
                output_stream: OutputStream::Stdout,
                output_path: "output".into(),
            },
            CommandErrorReason::OutputFilesConflict {
                stdout_path: "stdout".into(),
                stderr_path: "stderr".into(),
            },
            CommandErrorReason::InspectIoFileFailed {
                path: "output".into(),
                source: io_error(),
            },
            CommandErrorReason::StartInputThreadFailed { source: io_error() },
            CommandErrorReason::StartOutputThreadFailed {
                stream: OutputStream::Stderr,
                source: io_error(),
            },
            CommandErrorReason::TimeFailed {
                source: TimeError::TimerUnavailable {
                    source: TimerUnavailableError::BackendUnavailable {
                        backend: "test",
                        source: Box::new(io_error()),
                    },
                },
            },
            CommandErrorReason::WriteInputFailed { source: io_error() },
            CommandErrorReason::WriteOutputFailed {
                stream: OutputStream::Stdout,
                path: "output".into(),
                source: io_error(),
            },
            CommandErrorReason::TimedOut {
                timeout: Duration::from_secs(1),
            },
            CommandErrorReason::Cancelled,
            CommandErrorReason::CancelFailed {
                process_tree_source: io_error(),
                child_source: io_error(),
            },
            CommandErrorReason::OutputTruncated,
            CommandErrorReason::UnexpectedExit {
                exit_code: Some(9),
                expected: vec![0],
            },
        ];

        for reason in reasons {
            let reason_debug = format!("{reason:?}");
            let error = CommandError::from_reason("command", reason, None);
            let display = error.to_string();
            let debug = format!("{error:?}");

            assert!(!reason_debug.is_empty());
            assert!(display.contains("command"));
            assert!(debug.contains(&display));
            let _ = error.source();
            let _ = error.kind();
            let _ = error.reason();
            let _ = error.output();
            let _ = error.exit_code();
            let _ = error.is_unexpected_exit();
        }
    }

    #[test]
    fn test_command_error_formats_and_exposes_every_cleanup_variant() {
        let error = CommandError::from_reason("command", CommandErrorReason::Cancelled, None)
            .with_cleanup_failures([
                CommandCleanupFailure::Wait { source: io_error() },
                CommandCleanupFailure::ProcessTreeTermination { source: io_error() },
                CommandCleanupFailure::ChildTermination { source: io_error() },
                CommandCleanupFailure::Stdin { source: io_error() },
                CommandCleanupFailure::StdinCancellation { source: io_error() },
                CommandCleanupFailure::StdoutCancellation { source: io_error() },
                CommandCleanupFailure::StderrCancellation { source: io_error() },
                CommandCleanupFailure::StdoutRead { source: io_error() },
                CommandCleanupFailure::StdoutWrite {
                    path: "stdout".into(),
                    source: io_error(),
                },
                CommandCleanupFailure::StderrRead { source: io_error() },
                CommandCleanupFailure::StderrWrite {
                    path: "stderr".into(),
                    source: io_error(),
                },
            ]);

        assert_eq!(error.cleanup_failures().len(), 11);
        assert!(error.process_tree_source().is_some());
        assert!(error.child_source().is_some());
        assert!(error.to_string().contains("11 cleanup failure(s)"));
        for failure in error.cleanup_failures() {
            assert!(!format!("{failure:?}").is_empty());
        }
    }
}
