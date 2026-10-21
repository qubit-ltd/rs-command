// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for command error reason contracts.

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

use crate::CommandCleanupFailure;
use crate::CommandError;
use crate::CommandErrorKind;
use crate::CommandErrorReason;
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
            expected_display:
                "failed to open stdin file `<redacted path>` for command `command`: injected error source".into(),
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
            reason: CommandErrorReason::NonRegularInputFile { path: "input".into() },
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
            expected_display:
                "failed to open stdout file `<redacted path>` for command `command`: injected error source".into(),
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
            expected_display:
                "stdin file '<redacted path>' conflicts with stdout file '<redacted path>' for command 'command'".into(),
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
            expected_display:
                "stdout file '<redacted path>' conflicts with stderr file '<redacted path>' for command 'command'"
                    .into(),
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
            expected_display:
                "failed to inspect I/O file '<redacted path>' for command 'command': injected error source".into(),
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
            reason: CommandErrorReason::TimeFailed { source: time_source },
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
            expected_display:
                "failed to write stdout for command `command` to `<redacted path>`: injected error source".into(),
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
        CommandCleanupFailure::Time {
            source: TimeError::InstantOverflow,
        },
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

    assert_eq!(error.cleanup_failures().len(), 12);
    assert!(error.process_tree_source().is_some());
    assert!(error.child_source().is_some());
    assert!(error.to_string().contains("12 cleanup failure(s)"));
    for failure in error.cleanup_failures() {
        assert!(!format!("{failure:?}").is_empty());
    }
}
