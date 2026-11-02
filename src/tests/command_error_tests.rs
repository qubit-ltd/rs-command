// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests crate-internal command error finalization contracts.

use std::io;
use std::process::ExitStatus;
use std::time::Duration;

use qubit_clock::TimeError;

use crate::CommandCapturedOutput;
use crate::CommandCleanupFailure;
use crate::CommandError;
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
fn test_finalization_preserves_independent_captured_output() {
    for reason in [
        CommandErrorReason::Cancelled,
        CommandErrorReason::TimedOut {
            timeout: Duration::from_secs(1),
        },
    ] {
        let primary = CommandError::from_reason("command", reason, None);
        let expected_kind = primary.kind();
        let captured = CommandCapturedOutput::new((b"prefix".to_vec(), true, false), (b"err".to_vec(), false, true));
        let secondary = CommandError::from_reason(
            "command",
            CommandErrorReason::TimeFailed {
                source: TimeError::InstantOverflow,
            },
            None,
        )
        .with_captured_output(captured);
        let error = primary.with_finalization_error(secondary);
        assert_eq!(error.kind(), expected_kind);
        assert!(error.output().is_none());
        let output = error.captured_output().expect("captured bytes must survive");
        assert_eq!(output.stdout(), b"prefix");
        assert_eq!(output.stderr(), b"err");
        assert!(output.stdout_truncated());
        assert!(!output.stderr_truncated());
        assert!(!output.stdout_complete());
        assert!(output.stderr_complete());
        assert!(matches!(error.cleanup_failures(), [CommandCleanupFailure::Time { .. }]));
    }
}

#[test]
fn test_finalization_keeps_primary_independent_captured_output() {
    let primary_capture = CommandCapturedOutput::new((b"primary".to_vec(), false, true), (b"p".to_vec(), true, false));
    let primary =
        CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_captured_output(primary_capture);
    let secondary_output = CommandOutput::new(
        status(),
        (b"secondary".to_vec(), false, false),
        (Vec::new(), false, true),
        Duration::from_secs(1),
    );
    let secondary = CommandError::from_reason(
        "command",
        CommandErrorReason::TimeFailed {
            source: TimeError::InstantOverflow,
        },
        None,
    )
    .with_output(secondary_output)
    .with_captured_output(CommandCapturedOutput::new(
        (b"other".to_vec(), false, true),
        (Vec::new(), false, true),
    ));

    let error = primary.with_finalization_error(secondary);
    assert_eq!(error.kind(), CommandErrorKind::Cancelled);
    assert!(error.output().is_none());
    let captured = error.captured_output().expect("primary capture must survive");
    assert_eq!(captured.stdout(), b"primary");
    assert_eq!(captured.stderr(), b"p");
    assert!(!captured.stdout_truncated());
    assert!(captured.stderr_truncated());
    assert!(captured.stdout_complete());
    assert!(!captured.stderr_complete());
    assert!(matches!(error.cleanup_failures(), [CommandCleanupFailure::Time { .. }]));
}

#[test]
fn test_finalization_keeps_primary_capture_when_secondary_has_only_capture() {
    let primary = CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_captured_output(
        CommandCapturedOutput::new((b"primary".to_vec(), false, true), (b"p".to_vec(), true, false)),
    );
    let secondary = CommandError::from_reason(
        "command",
        CommandErrorReason::TimeFailed {
            source: TimeError::InstantOverflow,
        },
        None,
    )
    .with_captured_output(CommandCapturedOutput::new(
        (b"secondary".to_vec(), true, false),
        (b"s".to_vec(), false, true),
    ));

    let error = primary.with_finalization_error(secondary);
    assert_eq!(error.kind(), CommandErrorKind::Cancelled);
    assert!(error.output().is_none());
    let captured = error.captured_output().expect("primary capture must survive");
    assert_eq!(captured.stdout(), b"primary");
    assert_eq!(captured.stderr(), b"p");
    assert!(!captured.stdout_truncated());
    assert!(captured.stderr_truncated());
    assert!(captured.stdout_complete());
    assert!(!captured.stderr_complete());
    assert!(matches!(error.cleanup_failures(), [CommandCleanupFailure::Time { .. }]));
}

#[test]
fn test_finalization_keeps_primary_capture_when_secondary_has_only_output() {
    let primary = CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_captured_output(
        CommandCapturedOutput::new((b"primary".to_vec(), false, true), (b"p".to_vec(), true, false)),
    );
    let secondary_output = CommandOutput::new(
        status(),
        (b"secondary".to_vec(), false, false),
        (Vec::new(), false, true),
        Duration::from_secs(1),
    );
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
    assert!(error.output().is_none());
    let captured = error.captured_output().expect("primary capture must survive");
    assert_eq!(captured.stdout(), b"primary");
    assert_eq!(captured.stderr(), b"p");
    assert!(!captured.stdout_truncated());
    assert!(captured.stderr_truncated());
    assert!(captured.stdout_complete());
    assert!(!captured.stderr_complete());
    assert!(matches!(
        error.cleanup_failures(),
        [CommandCleanupFailure::StdoutRead { .. }]
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
    let primary = CommandError::from_reason("command", CommandErrorReason::Cancelled, Some(Box::new(primary_output)));
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
