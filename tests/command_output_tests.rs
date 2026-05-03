/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for [`CommandOutput`](qubit_command::CommandOutput).

#![cfg(not(windows))]

use qubit_command::{
    Command,
    CommandRunner,
};

#[test]
fn test_command_output_stdout_returns_bytes_and_text() {
    let output = CommandRunner::new()
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
    let output = CommandRunner::new()
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
fn test_command_output_rejects_invalid_utf8_for_strict_text() {
    let output = CommandRunner::new()
        .run(Command::shell("printf '\\377'; printf '\\377' >&2"))
        .expect("command should run successfully");

    assert!(output.stdout_text().is_err());
    assert!(output.stderr_text().is_err());
    assert_eq!(output.stdout(), &[0xff]);
    assert_eq!(output.stderr(), &[0xff]);
}

#[test]
fn test_command_output_always_exposes_lossy_text() {
    let output = CommandRunner::new()
        .run(Command::shell("printf '\\377'; printf '\\377' >&2"))
        .expect("command should run successfully");

    assert_eq!(output.stdout_lossy_text(), "\u{fffd}");
    assert_eq!(output.stderr_lossy_text(), "\u{fffd}");
    assert_eq!(output.stdout(), &[0xff]);
    assert_eq!(output.stderr(), &[0xff]);
}

#[test]
fn test_command_output_reports_unix_termination_signal() {
    let error = CommandRunner::new()
        .run(Command::shell("kill -TERM $$"))
        .expect_err("signal-terminated command should not be successful");
    let output = error
        .output()
        .expect("unexpected exit should expose output");

    assert_eq!(output.exit_code(), None);
    assert_eq!(output.termination_signal(), Some(15));
}
