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
fn test_command_output_stdout_returns_utf8_text_and_bytes() {
    let output = CommandRunner::new()
        .run(Command::shell("printf hello"))
        .expect("command should run successfully");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "hello"
    );
    assert_eq!(output.exit_status().code(), Some(0));
    assert_eq!(output.stdout_bytes(), b"hello");
    assert!(!output.stdout_truncated());
}

#[test]
fn test_command_output_stderr_returns_utf8_text_and_bytes() {
    let output = CommandRunner::new()
        .run(Command::shell("printf error >&2"))
        .expect("command should run successfully");

    assert_eq!(
        output.stderr().expect("stderr should be valid UTF-8"),
        "error"
    );
    assert_eq!(output.stderr_bytes(), b"error");
    assert!(!output.stderr_truncated());
}

#[test]
fn test_command_output_rejects_invalid_utf8_by_default() {
    let output = CommandRunner::new()
        .run(Command::shell("printf '\\377'; printf '\\377' >&2"))
        .expect("command should run successfully");

    assert!(output.stdout().is_err());
    assert!(output.stderr().is_err());
    assert_eq!(output.stdout_bytes(), &[0xff]);
    assert_eq!(output.stderr_bytes(), &[0xff]);
}

#[test]
fn test_command_output_uses_lossy_text_when_configured() {
    let output = CommandRunner::new()
        .lossy_output(true)
        .run(Command::shell("printf '\\377'; printf '\\377' >&2"))
        .expect("command should run successfully");

    assert_eq!(
        output.stdout().expect("stdout should be lossy UTF-8"),
        "\u{fffd}"
    );
    assert_eq!(
        output.stderr().expect("stderr should be lossy UTF-8"),
        "\u{fffd}"
    );
    assert_eq!(output.stdout_bytes(), &[0xff]);
    assert_eq!(output.stderr_bytes(), &[0xff]);
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
