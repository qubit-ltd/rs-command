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
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::time::Duration;

    use qubit_clock::TimeError;
    use qubit_clock::TimerUnavailableError;

    use super::CommandErrorReason;
    use crate::CommandCleanupFailure;
    use crate::CommandError;
    use crate::CommandErrorKind;
    use crate::CommandOutput;
    use crate::OutputStream;

    struct ErrorCase {
        label: &'static str,
        reason: CommandErrorReason,
        reason_debug_key: &'static str,
        expected_display: String,
        expected_kind: CommandErrorKind,
        has_source: bool,
        retains_output: bool,
        expected_exit_code: Option<i32>,
        is_unexpected_exit: bool,
        has_process_tree_source: bool,
        has_child_source: bool,
    }

    fn io_error() -> io::Error {
        io::Error::other("injected error source")
    }

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code as u32)
    }

    fn output() -> CommandOutput {
        CommandOutput::new(
            status(23),
            (b"stdout".to_vec(), false, true),
            (b"stderr".to_vec(), false, true),
            Duration::from_secs(2),
        )
    }

    #[test]
    fn test_command_error_enforces_every_reason_contract() {
        let time_source = TimeError::TimerUnavailable {
            source: TimerUnavailableError::BackendUnavailable {
                backend: "test",
                source: Box::new(io_error()),
            },
        };
        let time_display = format!("time handling failed for command 'command': {time_source}");
        let cases = vec![
            ErrorCase {
                label: "SpawnFailed",
                reason: CommandErrorReason::SpawnFailed { source: io_error() },
                reason_debug_key: "SpawnFailed",
                expected_display: "failed to spawn command `command`: injected error source".into(),
                expected_kind: CommandErrorKind::SpawnFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "WaitFailed",
                reason: CommandErrorReason::WaitFailed { source: io_error() },
                reason_debug_key: "WaitFailed",
                expected_display: "failed to wait for command `command`: injected error source".into(),
                expected_kind: CommandErrorKind::WaitFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "CancelledBeforeStart",
                reason: CommandErrorReason::CancelledBeforeStart,
                reason_debug_key: "CancelledBeforeStart",
                expected_display: "command `command` was cancelled before it started".into(),
                expected_kind: CommandErrorKind::CancelledBeforeStart,
                has_source: false,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "KillFailed",
                reason: CommandErrorReason::KillFailed {
                    timeout: Duration::from_secs(1),
                    process_tree_source: io_error(),
                    child_source: io_error(),
                },
                reason_debug_key: "KillFailed",
                expected_display: "failed to terminate timed-out command `command` after 1s; process-tree source: injected error source; child source: injected error source".into(),
                expected_kind: CommandErrorKind::KillFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: true,
                has_child_source: true,
            },
            ErrorCase {
                label: "ReadOutputFailed",
                reason: CommandErrorReason::ReadOutputFailed {
                    stream: OutputStream::Stdout,
                    source: io_error(),
                },
                reason_debug_key: "ReadOutputFailed",
                expected_display: "failed to read stdout for command `command`: injected error source".into(),
                expected_kind: CommandErrorKind::ReadOutputFailed,
                has_source: true,
                retains_output: true,
                expected_exit_code: Some(23),
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "OpenInputFailed",
                reason: CommandErrorReason::OpenInputFailed {
                    path: "input".into(),
                    source: io_error(),
                },
                reason_debug_key: "OpenInputFailed",
                expected_display: "failed to open stdin file `<redacted path>` for command `command`: injected error source".into(),
                expected_kind: CommandErrorKind::OpenInputFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "NonRegularInputFile",
                reason: CommandErrorReason::NonRegularInputFile {
                    path: "input".into(),
                },
                reason_debug_key: "NonRegularInputFile",
                expected_display: "stdin path `<redacted path>` for command `command` is not an ordinary file".into(),
                expected_kind: CommandErrorKind::NonRegularInputFile,
                has_source: false,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "OpenOutputFailed",
                reason: CommandErrorReason::OpenOutputFailed {
                    stream: OutputStream::Stdout,
                    path: "output".into(),
                    source: io_error(),
                },
                reason_debug_key: "OpenOutputFailed",
                expected_display: "failed to open stdout file `<redacted path>` for command `command`: injected error source".into(),
                expected_kind: CommandErrorKind::OpenOutputFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "NonRegularOutputFile",
                reason: CommandErrorReason::NonRegularOutputFile {
                    stream: OutputStream::Stderr,
                    path: "output".into(),
                },
                reason_debug_key: "NonRegularOutputFile",
                expected_display: "stderr path `<redacted path>` for command `command` is not an ordinary file".into(),
                expected_kind: CommandErrorKind::NonRegularOutputFile,
                has_source: false,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "InputOutputConflict",
                reason: CommandErrorReason::InputOutputConflict {
                    input_path: "input".into(),
                    output_stream: OutputStream::Stdout,
                    output_path: "output".into(),
                },
                reason_debug_key: "InputOutputConflict",
                expected_display: "stdin file '<redacted path>' conflicts with stdout file '<redacted path>' for command 'command'".into(),
                expected_kind: CommandErrorKind::InputOutputConflict,
                has_source: false,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "OutputFilesConflict",
                reason: CommandErrorReason::OutputFilesConflict {
                    stdout_path: "stdout".into(),
                    stderr_path: "stderr".into(),
                },
                reason_debug_key: "OutputFilesConflict",
                expected_display: "stdout file '<redacted path>' conflicts with stderr file '<redacted path>' for command 'command'".into(),
                expected_kind: CommandErrorKind::OutputFilesConflict,
                has_source: false,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "InspectIoFileFailed",
                reason: CommandErrorReason::InspectIoFileFailed {
                    path: "output".into(),
                    source: io_error(),
                },
                reason_debug_key: "InspectIoFileFailed",
                expected_display: "failed to inspect I/O file '<redacted path>' for command 'command': injected error source".into(),
                expected_kind: CommandErrorKind::InspectIoFileFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "StartInputThreadFailed",
                reason: CommandErrorReason::StartInputThreadFailed { source: io_error() },
                reason_debug_key: "StartInputThreadFailed",
                expected_display: "failed to start stdin writer for command 'command': injected error source".into(),
                expected_kind: CommandErrorKind::StartInputThreadFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "StartOutputThreadFailed",
                reason: CommandErrorReason::StartOutputThreadFailed {
                    stream: OutputStream::Stderr,
                    source: io_error(),
                },
                reason_debug_key: "StartOutputThreadFailed",
                expected_display: "failed to start stderr reader for command 'command': injected error source".into(),
                expected_kind: CommandErrorKind::StartOutputThreadFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "TimeFailed",
                reason: CommandErrorReason::TimeFailed {
                    source: time_source,
                },
                reason_debug_key: "TimeFailed",
                expected_display: time_display,
                expected_kind: CommandErrorKind::TimeFailed,
                has_source: true,
                retains_output: true,
                expected_exit_code: Some(23),
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "WriteInputFailed",
                reason: CommandErrorReason::WriteInputFailed { source: io_error() },
                reason_debug_key: "WriteInputFailed",
                expected_display: "failed to write stdin for command `command`: injected error source".into(),
                expected_kind: CommandErrorKind::WriteInputFailed,
                has_source: true,
                retains_output: true,
                expected_exit_code: Some(23),
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "WriteOutputFailed",
                reason: CommandErrorReason::WriteOutputFailed {
                    stream: OutputStream::Stdout,
                    path: "output".into(),
                    source: io_error(),
                },
                reason_debug_key: "WriteOutputFailed",
                expected_display: "failed to write stdout for command `command` to `<redacted path>`: injected error source".into(),
                expected_kind: CommandErrorKind::WriteOutputFailed,
                has_source: true,
                retains_output: true,
                expected_exit_code: Some(23),
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "TimedOut",
                reason: CommandErrorReason::TimedOut {
                    timeout: Duration::from_secs(1),
                },
                reason_debug_key: "TimedOut",
                expected_display: "command `command` timed out after 1s".into(),
                expected_kind: CommandErrorKind::TimedOut,
                has_source: false,
                retains_output: true,
                expected_exit_code: Some(23),
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "Cancelled",
                reason: CommandErrorReason::Cancelled,
                reason_debug_key: "Cancelled",
                expected_display: "command `command` was cancelled".into(),
                expected_kind: CommandErrorKind::Cancelled,
                has_source: false,
                retains_output: true,
                expected_exit_code: Some(23),
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "CancelFailed",
                reason: CommandErrorReason::CancelFailed {
                    process_tree_source: io_error(),
                    child_source: io_error(),
                },
                reason_debug_key: "CancelFailed",
                expected_display: "failed to cancel command `command`; process-tree source: injected error source; child source: injected error source".into(),
                expected_kind: CommandErrorKind::CancelFailed,
                has_source: true,
                retains_output: false,
                expected_exit_code: None,
                is_unexpected_exit: false,
                has_process_tree_source: true,
                has_child_source: true,
            },
            ErrorCase {
                label: "OutputTruncated",
                reason: CommandErrorReason::OutputTruncated,
                reason_debug_key: "OutputTruncated",
                expected_display: "command `command` completed successfully, but captured output was truncated".into(),
                expected_kind: CommandErrorKind::OutputTruncated,
                has_source: false,
                retains_output: true,
                expected_exit_code: Some(23),
                is_unexpected_exit: false,
                has_process_tree_source: false,
                has_child_source: false,
            },
            ErrorCase {
                label: "UnexpectedExit",
                reason: CommandErrorReason::UnexpectedExit {
                    exit_code: Some(9),
                    expected: vec![0],
                },
                reason_debug_key: "UnexpectedExit",
                expected_display: "command `command` exited with code Some(9); expected one of [0]".into(),
                expected_kind: CommandErrorKind::UnexpectedExit,
                has_source: false,
                retains_output: true,
                expected_exit_code: Some(9),
                is_unexpected_exit: true,
                has_process_tree_source: false,
                has_child_source: false,
            },
        ];

        for case in cases {
            let case_output = case.retains_output.then(|| Box::new(output()));
            let reason_debug = format!("{:?}", case.reason);
            let error = CommandError::from_reason("command", case.reason, case_output);

            assert!(
                reason_debug.starts_with(case.reason_debug_key),
                "{} reason Debug contract changed: {reason_debug}",
                case.label,
            );
            assert_eq!(error.command(), "command", "{} command", case.label);
            assert_eq!(error.kind(), case.expected_kind, "{} kind", case.label);
            assert_eq!(error.to_string(), case.expected_display, "{} Display", case.label);
            assert_eq!(
                format!("{error:?}"),
                format!("CommandError {{ message: {:?} }}", case.expected_display),
                "{} Debug",
                case.label,
            );
            assert_eq!(error.source().is_some(), case.has_source, "{} source", case.label);
            assert_eq!(error.output().is_some(), case.retains_output, "{} output", case.label,);
            assert_eq!(error.exit_code(), case.expected_exit_code, "{} exit code", case.label);
            assert_eq!(
                error.is_unexpected_exit(),
                case.is_unexpected_exit,
                "{} unexpected exit",
                case.label,
            );
            assert_eq!(
                error.process_tree_source().is_some(),
                case.has_process_tree_source,
                "{} process-tree source",
                case.label,
            );
            assert_eq!(
                error.child_source().is_some(),
                case.has_child_source,
                "{} child source",
                case.label,
            );
        }
    }

    #[test]
    fn test_command_error_formats_and_exposes_every_cleanup_variant() {
        let error = CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_cleanup_failures([
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
