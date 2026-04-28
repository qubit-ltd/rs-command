/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for captured output behavior.

use qubit_command::{
    Command,
    CommandRunner,
};

#[test]
fn test_captured_output_records_stdout_truncation() {
    let output = CommandRunner::new()
        .max_stdout_bytes(5)
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert_eq!(output.stdout_bytes().len(), 5);
    assert!(output.stdout_truncated());
}

#[test]
fn test_captured_output_can_keep_zero_stdout_bytes() {
    let output = CommandRunner::new()
        .max_stdout_bytes(0)
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert!(output.stdout_bytes().is_empty());
    assert!(output.stdout_truncated());
}

#[test]
fn test_captured_output_limit_without_truncation() {
    let output = CommandRunner::new()
        .max_stdout_bytes(1024)
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert!(output.stdout_bytes().starts_with(b"rustc "));
    assert!(!output.stdout_truncated());
}
