/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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

    assert!(output.stdout_bytes().starts_with(b"rustc "));
}

#[cfg(not(windows))]
#[test]
fn test_command_stdin_bytes_reaches_child_process() {
    let output = CommandRunner::new()
        .run(Command::new("cat").stdin_bytes(b"input".to_vec()))
        .expect("stdin bytes should be written to child");

    assert_eq!(output.stdout_bytes(), b"input");
}
