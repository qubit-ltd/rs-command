// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for captured output behavior.

use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandRunner;

#[test]
fn test_captured_output_records_stdout_truncation() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .max_stdout_bytes(5)
        .fail_on_output_truncation(false)
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert_eq!(output.stdout().len(), 5, "stdout should be limited to five bytes");
    assert!(output.stdout_truncated(), "stdout should report truncation");
}

#[test]
fn test_captured_output_can_keep_zero_stdout() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .max_stdout_bytes(0)
        .fail_on_output_truncation(false)
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert!(
        output.stdout().is_empty(),
        "stdout should be empty when its limit is zero"
    );
    assert!(output.stdout_truncated(), "stdout should report discarded output");
}

#[test]
fn test_captured_output_limit_without_truncation() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .max_stdout_bytes(1024)
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert!(
        output.stdout().starts_with(b"rustc "),
        "stdout should contain the rustc version"
    );
    assert!(
        !output.stdout_truncated(),
        "stdout should not be truncated below its limit"
    );
}
