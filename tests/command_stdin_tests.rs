/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for command stdin behavior.

use qubit_command::{
    Command,
    CommandRunner,
};

#[test]
fn test_command_stdin_null_allows_command_without_input() {
    let output = CommandRunner::new()
        .run(Command::new("rustc").arg("--version").stdin_null())
        .expect("command with null stdin should run successfully");

    assert!(output.stdout().starts_with(b"rustc "));
}

#[cfg(not(windows))]
#[test]
fn test_command_stdin_bytes_reaches_child_process() {
    let output = CommandRunner::new()
        .run(Command::new("cat").stdin_bytes(b"input".to_vec()))
        .expect("stdin bytes should be written to child");

    assert_eq!(output.stdout(), b"input");
}
