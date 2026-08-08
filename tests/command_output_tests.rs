// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandOutput`](qubit_command::CommandOutput).

#![cfg(not(windows))]

use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandRunner;

#[test]
fn test_command_output_stdout_returns_bytes_and_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("printf hello"))
        .expect("command should run successfully");

    assert_eq!(output.stdout(), b"hello");
    assert_eq!(
        output.stdout_text().expect("stdout should be valid UTF-8"),
        "hello"
    );
    assert_eq!(output.stdout_lossy_text(), "hello");
    assert_eq!(output.exit_status().code(), Some(0));
    assert!(!output.stdout_truncated());
}

#[test]
fn test_command_output_stderr_returns_bytes_and_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("printf error >&2"))
        .expect("command should run successfully");

    assert_eq!(output.stderr(), b"error");
    assert_eq!(
        output.stderr_text().expect("stderr should be valid UTF-8"),
        "error"
    );
    assert_eq!(output.stderr_lossy_text(), "error");
    assert!(!output.stderr_truncated());
}

#[test]
fn test_command_output_consuming_stream_accessors_preserve_bytes() {
    let stdout = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("printf stdout"))
        .expect("command should run successfully")
        .into_stdout();
    let stderr = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("printf stderr >&2"))
        .expect("command should run successfully")
        .into_stderr();

    assert_eq!(stdout, b"stdout");
    assert_eq!(stderr, b"stderr");
}

#[test]
fn test_command_output_rejects_invalid_utf8_for_strict_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("printf '\\377'; printf '\\377' >&2"))
        .expect("command should run successfully");

    assert!(output.stdout_text().is_err());
    assert!(output.stderr_text().is_err());
    assert_eq!(output.stdout(), &[0xff]);
    assert_eq!(output.stderr(), &[0xff]);
}

#[test]
fn test_command_output_always_exposes_lossy_text() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("printf '\\377'; printf '\\377' >&2"))
        .expect("command should run successfully");

    assert_eq!(output.stdout_lossy_text(), "\u{fffd}");
    assert_eq!(output.stderr_lossy_text(), "\u{fffd}");
    assert_eq!(output.stdout(), &[0xff]);
    assert_eq!(output.stderr(), &[0xff]);
}

#[test]
fn test_command_output_reports_unix_termination_signal() {
    let error = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("kill -TERM $$"))
        .expect_err("signal-terminated command should not be successful");
    let output = error
        .output()
        .expect("unexpected exit should expose output");

    assert_eq!(output.exit_code(), None);
    assert_eq!(output.termination_signal(), Some(15));
}

#[test]
fn test_command_output_debug_redacts_captured_streams() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell(
            "printf stdout-secret; printf stderr-secret >&2",
        ))
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
