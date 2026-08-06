// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandError`](qubit_command::CommandError).

#![cfg(not(windows))]

use std::{
    io,
    path::PathBuf,
    time::Duration,
};

use qubit_clock::{
    TimeError,
    TimerUnavailableError,
};
use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
    OutputStream,
};

#[test]
fn test_command_error_accessors_for_errors_without_output() {
    let spawn = CommandError::SpawnFailed {
        command: "missing".to_owned(),
        source: io::Error::new(io::ErrorKind::NotFound, "missing"),
    };
    assert_eq!(spawn.command(), "missing");
    assert!(spawn.output().is_none());
    assert!(spawn.to_string().contains("failed to spawn command"));

    let wait = CommandError::WaitFailed {
        command: "wait".to_owned(),
        source: io::Error::other("wait failed"),
    };
    assert_eq!(wait.command(), "wait");
    assert!(wait.output().is_none());
    assert!(wait.to_string().contains("failed to wait"));

    let cancelled_before_start = CommandError::CancelledBeforeStart {
        command: "cancelled-before-start".to_owned(),
    };
    assert_eq!(cancelled_before_start.command(), "cancelled-before-start");
    assert!(cancelled_before_start.output().is_none());
    assert!(
        cancelled_before_start
            .to_string()
            .contains("cancelled before it started")
    );

    let kill = CommandError::KillFailed {
        command: "kill".to_owned(),
        timeout: Duration::from_secs(1),
        source: io::Error::other("kill failed"),
    };
    assert_eq!(kill.command(), "kill");
    assert!(kill.output().is_none());
    assert!(kill.to_string().contains("failed to kill"));

    let read = CommandError::ReadOutputFailed {
        command: "read".to_owned(),
        stream: OutputStream::Stdout,
        source: io::Error::other("read failed"),
    };
    assert_eq!(read.command(), "read");
    assert!(read.output().is_none());
    assert!(read.to_string().contains("failed to read stdout"));

    let open_input = CommandError::OpenInputFailed {
        command: "open-input".to_owned(),
        path: PathBuf::from("stdin.txt"),
        source: io::Error::other("open input failed"),
    };
    assert_eq!(open_input.command(), "open-input");
    assert!(open_input.output().is_none());
    assert!(open_input.to_string().contains("failed to open stdin file"));

    let open_output = CommandError::OpenOutputFailed {
        command: "open-output".to_owned(),
        stream: OutputStream::Stderr,
        path: PathBuf::from("stderr.txt"),
        source: io::Error::other("open output failed"),
    };
    assert_eq!(open_output.command(), "open-output");
    assert!(open_output.output().is_none());
    assert!(
        open_output
            .to_string()
            .contains("failed to open stderr file")
    );

    let write_input = CommandError::WriteInputFailed {
        command: "write-input".to_owned(),
        source: io::Error::other("write input failed"),
    };
    assert_eq!(write_input.command(), "write-input");
    assert!(write_input.output().is_none());
    assert!(write_input.to_string().contains("failed to write stdin"));

    let write_output = CommandError::WriteOutputFailed {
        command: "write-output".to_owned(),
        stream: OutputStream::Stdout,
        path: PathBuf::from("stdout.txt"),
        source: io::Error::other("write output failed"),
        output: None,
    };
    assert_eq!(write_output.command(), "write-output");
    assert!(write_output.output().is_none());
    assert!(write_output.to_string().contains("failed to write stdout"));
}

#[test]
fn test_command_error_accessors_for_preparation_and_time_errors() {
    let input_output = CommandError::InputOutputConflict {
        command: "input-output".to_owned(),
        input_path: PathBuf::from("input.txt"),
        output_stream: OutputStream::Stdout,
        output_path: PathBuf::from("output.txt"),
    };
    assert_eq!(input_output.command(), "input-output");
    assert!(input_output.output().is_none());
    assert!(input_output.to_string().contains("conflicts with stdout"));

    let output_files = CommandError::OutputFilesConflict {
        command: "output-files".to_owned(),
        stdout_path: PathBuf::from("stdout.txt"),
        stderr_path: PathBuf::from("stderr.txt"),
    };
    assert_eq!(output_files.command(), "output-files");
    assert!(output_files.output().is_none());
    assert!(
        output_files
            .to_string()
            .contains("conflicts with stderr file")
    );

    let inspect = CommandError::InspectIoFileFailed {
        command: "inspect".to_owned(),
        path: PathBuf::from("loop"),
        source: io::Error::other("inspection failed"),
    };
    assert_eq!(inspect.command(), "inspect");
    assert!(inspect.output().is_none());
    assert!(inspect.to_string().contains("failed to inspect I/O file"));

    let time = CommandError::TimeFailed {
        command: "time".to_owned(),
        source: TimeError::TimerUnavailable {
            source: TimerUnavailableError::BackendUnavailable {
                backend: "test",
                source: Box::new(io::Error::other(
                    "test timer backend unavailable",
                )),
            },
        },
    };
    assert_eq!(time.command(), "time");
    assert!(time.output().is_none());
    assert!(time.to_string().contains("time handling failed"));
}

#[test]
fn test_start_output_thread_error_reports_command_and_stream() {
    let error = CommandError::StartOutputThreadFailed {
        command: "tool <redacted>".to_owned(),
        stream: OutputStream::Stdout,
        source: io::Error::other("thread unavailable"),
    };

    assert_eq!(error.command(), "tool <redacted>");
    assert!(error.output().is_none());
    assert!(error.to_string().contains("stdout"));
}

#[test]
fn test_start_input_thread_error_reports_command() {
    let error = CommandError::StartInputThreadFailed {
        command: "tool <redacted>".to_owned(),
        source: io::Error::other("thread unavailable"),
    };

    assert_eq!(error.command(), "tool <redacted>");
    assert!(error.output().is_none());
    assert!(error.to_string().contains("stdin"));
}

#[test]
fn test_command_error_accessors_for_errors_with_output() {
    let truncated = CommandRunner::new()
        .max_stdout_bytes(3)
        .fail_on_output_truncation(true)
        .run(Command::shell("printf output"))
        .expect_err("truncated output should be rejected");
    assert!(matches!(truncated, CommandError::OutputTruncated { .. }));
    assert_eq!(truncated.command(), r#"["sh", "-c", "<redacted>"]"#);
    assert_eq!(
        truncated
            .output()
            .expect("truncation error should expose output")
            .stdout(),
        b"out",
    );

    let unexpected = CommandRunner::new()
        .run(Command::shell("printf output; exit 9"))
        .expect_err("non-success exit code should be rejected");
    assert_eq!(unexpected.command(), r#"["sh", "-c", "<redacted>"]"#);
    assert_eq!(
        unexpected
            .output()
            .expect("unexpected exit should expose output")
            .stdout_text()
            .expect("stdout should be valid UTF-8"),
        "output",
    );

    let timed_out = CommandRunner::new()
        .timeout(Duration::from_millis(500))
        .run(Command::shell("printf before-timeout; sleep 2"))
        .expect_err("long-running command should time out");
    assert_eq!(timed_out.command(), r#"["sh", "-c", "<redacted>"]"#);
    assert_eq!(
        timed_out
            .output()
            .expect("timeout should expose captured output")
            .stdout_text()
            .expect("stdout should be valid UTF-8"),
        "before-timeout",
    );
}

#[test]
fn test_command_error_into_output_moves_retained_output() {
    let error = CommandRunner::new()
        .run(Command::shell("printf output; exit 9"))
        .expect_err("non-success exit code should be rejected");

    let output = error
        .into_output()
        .expect("unexpected exit should move retained output");

    assert_eq!(output.into_stdout(), b"output");
}

#[test]
fn test_command_error_accessors_cover_cancelled_and_tee_output() {
    let output = CommandRunner::new()
        .without_timeout()
        .cancellation_token(qubit_command::CommandCancellation::new())
        .run(Command::shell("printf output"))
        .expect("command should finish before cancellation");

    let cancelled = CommandError::Cancelled {
        command: "cancelled".to_owned(),
        output: Box::new(output.clone()),
    };
    assert_eq!(cancelled.command(), "cancelled");
    assert!(cancelled.output().is_some());
    assert!(cancelled.into_output().is_some());

    let tee = CommandError::WriteOutputFailed {
        command: "tee".to_owned(),
        stream: OutputStream::Stdout,
        path: PathBuf::from("tee.log"),
        source: io::Error::other("tee failed"),
        output: Some(Box::new(output)),
    };
    assert!(tee.output().is_some());
    assert!(tee.into_output().is_some());

    let cancelled_before_start = CommandError::CancelledBeforeStart {
        command: "cancelled-before-start".to_owned(),
    };
    assert_eq!(cancelled_before_start.command(), "cancelled-before-start");

    let cancel_failed = CommandError::CancelFailed {
        command: "cancel-failed".to_owned(),
        source: io::Error::other("cancel failed"),
    };
    assert_eq!(cancel_failed.command(), "cancel-failed");

    for error in [
        CommandError::TimedOut {
            command: "timed-out".to_owned(),
            timeout: Duration::from_secs(1),
            output: Box::new(
                CommandRunner::new()
                    .run(Command::shell("printf timed-out"))
                    .expect("timed-out fixture should run"),
            ),
        },
        CommandError::OutputTruncated {
            command: "truncated".to_owned(),
            output: Box::new(
                CommandRunner::new()
                    .run(Command::shell("printf truncated"))
                    .expect("truncation fixture should run"),
            ),
        },
    ] {
        assert!(error.into_output().is_some());
    }
}

#[test]
fn test_command_error_reports_unexpected_exit_signal() {
    let error = CommandRunner::new()
        .run(Command::shell("kill -TERM $$"))
        .expect_err("signal-terminated command should be rejected");

    assert!(matches!(error, CommandError::UnexpectedExit { .. }));
    assert!(error.to_string().contains("signal 15"));
}

#[test]
fn test_command_error_debug_does_not_expose_captured_streams() {
    let error = CommandRunner::new()
        .run(Command::shell(
            "printf stdout-secret; printf stderr-secret >&2; exit 7",
        ))
        .expect_err("command should fail");

    let debug = format!("{error:?}");
    let stdout_debug = format!("{:?}", b"stdout-secret".to_vec());
    let stderr_debug = format!("{:?}", b"stderr-secret".to_vec());
    assert!(!debug.contains("stdout-secret"));
    assert!(!debug.contains("stderr-secret"));
    assert!(!debug.contains(&stdout_debug));
    assert!(!debug.contains(&stderr_debug));
}
