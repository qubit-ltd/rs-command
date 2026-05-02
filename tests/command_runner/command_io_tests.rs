/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for command I/O collection behavior.

use qubit_command::{
    Command,
    CommandRunner,
};

#[test]
fn test_command_io_collects_stdout_and_stderr() {
    let output = CommandRunner::new()
        .run(Command::shell("rustc --version && rustc --version 1>&2"))
        .expect("shell command should run successfully");

    assert!(output.stdout_bytes().starts_with(b"rustc "));
    assert!(output.stderr_bytes().starts_with(b"rustc "));
}
