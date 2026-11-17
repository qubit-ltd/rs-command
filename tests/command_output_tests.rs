// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandOutput`](qubit_command::CommandOutput).

use std::time::Duration;

#[cfg(unix)]
use qubit_command::Command;
use qubit_command::CommandRunner;

mod support;
use support::output_command::output_command;

#[test]
fn test_command_output_stdout_returns_bytes_and_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"hello", b"", 0))
        .expect("command should run successfully");

    assert_eq!(output.stdout(), b"hello");
    assert_eq!(output.stdout_text().expect("stdout should be valid UTF-8"), "hello");
    assert_eq!(output.stdout_lossy_text(), "hello");
    assert_eq!(output.exit_status().code(), Some(0));
    assert!(!output.stdout_truncated());
}

#[test]
fn test_command_output_stderr_returns_bytes_and_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"", b"error", 0))
        .expect("command should run successfully");

    assert_eq!(output.stderr(), b"error");
    assert_eq!(output.stderr_text().expect("stderr should be valid UTF-8"), "error");
    assert_eq!(output.stderr_lossy_text(), "error");
    assert!(!output.stderr_truncated());
}

#[test]
fn test_command_output_consuming_stream_accessors_preserve_bytes() {
    let stdout = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"stdout", b"", 0))
        .expect("command should run successfully")
        .into_stdout();
    let stderr = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"", b"stderr", 0))
        .expect("command should run successfully")
        .into_stderr();

    assert_eq!(stdout, b"stdout");
    assert_eq!(stderr, b"stderr");
}

#[test]
fn test_command_output_into_captured_output_moves_both_streams_and_metadata() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"out", b"err", 0))
        .expect("command should run");
    let out_ptr = output.stdout().as_ptr();
    let err_ptr = output.stderr().as_ptr();

    let captured = output.into_captured_output();
    assert!(captured.stdout_complete());
    assert!(captured.stderr_complete());
    assert!(!captured.stdout_truncated());
    assert!(!captured.stderr_truncated());
    let (stdout, stderr) = captured.into_streams();
    assert_eq!(stdout, b"out");
    assert_eq!(stderr, b"err");
    assert_eq!(stdout.as_ptr(), out_ptr);
    assert_eq!(stderr.as_ptr(), err_ptr);
}

#[test]
fn test_command_captured_output_into_streams_preserves_empty_binary_and_truncated_bytes() {
    let empty = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"", b"", 0))
        .expect("empty command should run")
        .into_captured_output();
    assert!(empty.stdout_complete() && empty.stderr_complete());
    assert!(!empty.stdout_truncated() && !empty.stderr_truncated());
    let (stdout, stderr) = empty.into_streams();
    assert!(stdout.is_empty() && stderr.is_empty());

    let binary = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(&[0, 0xff], &[0x80, 0], 0))
        .expect("binary command should run")
        .into_captured_output();
    assert_eq!(binary.stdout(), &[0, 0xff]);
    assert_eq!(binary.stderr(), &[0x80, 0]);
    let (stdout, stderr) = binary.into_streams();
    assert_eq!(stdout, [0, 0xff]);
    assert_eq!(stderr, [0x80, 0]);

    let truncated = CommandRunner::new(Duration::from_secs(10))
        .max_output_bytes(1)
        .fail_on_output_truncation(false)
        .run(output_command(b"out", b"err", 0))
        .expect("accepted truncation should succeed")
        .into_captured_output();
    assert_eq!(truncated.stdout(), b"o");
    assert_eq!(truncated.stderr(), b"e");
    assert!(truncated.stdout_truncated() && truncated.stderr_truncated());
    assert!(truncated.stdout_complete() && truncated.stderr_complete());
    let (stdout, stderr) = truncated.into_streams();
    assert_eq!(stdout, b"o");
    assert_eq!(stderr, b"e");
}

#[test]
fn test_command_error_into_captured_output_into_streams_moves_both_streams() {
    let error = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"error-out", b"error-err", 9))
        .expect_err("non-zero exit should fail");
    let before = error.captured_output().expect("capture should be present");
    let out_ptr = before.stdout().as_ptr();
    let err_ptr = before.stderr().as_ptr();
    let captured = error.into_captured_output().expect("capture should move");
    let (stdout, stderr) = captured.into_streams();
    assert_eq!(stdout, b"error-out");
    assert_eq!(stderr, b"error-err");
    assert_eq!(stdout.as_ptr(), out_ptr);
    assert_eq!(stderr.as_ptr(), err_ptr);
}

#[test]
fn test_command_output_rejects_invalid_utf8_for_strict_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(&[0xff], &[0xff], 0))
        .expect("command should run successfully");

    assert!(output.stdout_text().is_err());
    assert!(output.stderr_text().is_err());
    assert_eq!(output.stdout(), &[0xff]);
    assert_eq!(output.stderr(), &[0xff]);
}

#[test]
fn test_command_output_always_exposes_lossy_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(&[0xff], &[0xff], 0))
        .expect("command should run successfully");

    assert_eq!(output.stdout_lossy_text(), "\u{fffd}");
    assert_eq!(output.stderr_lossy_text(), "\u{fffd}");
    assert_eq!(output.stdout(), &[0xff]);
    assert_eq!(output.stderr(), &[0xff]);
}

#[cfg(unix)]
#[test]
fn test_command_output_reports_unix_termination_signal() {
    let error = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("kill -TERM $$"))
        .expect_err("signal-terminated command should not be successful");
    let output = error.output().expect("unexpected exit should expose output");

    assert_eq!(output.exit_code(), None);
    assert_eq!(output.termination_signal(), Some(15));
}

#[test]
fn test_command_output_debug_redacts_captured_streams() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(output_command(b"stdout-secret", b"stderr-secret", 0))
        .expect("command should run successfully");

    let debug = format!("{output:?}");
    let stdout_debug = format!("{:?}", b"stdout-secret".to_vec());
    let stderr_debug = format!("{:?}", b"stderr-secret".to_vec());
    assert!(!debug.contains("stdout-secret"));
    assert!(!debug.contains("stderr-secret"));
    assert!(!debug.contains(&stdout_debug));
    assert!(!debug.contains(&stderr_debug));
    assert!(debug.contains("stdout_len"));
    assert!(debug.contains("stderr_len"));
    assert!(debug.contains("<redacted>"));
}
