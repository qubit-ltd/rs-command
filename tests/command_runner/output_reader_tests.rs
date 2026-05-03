/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for output reader behavior.

use qubit_command::{
    Command,
    CommandRunner,
};

#[test]
fn test_output_reader_drains_stderr_even_when_stdout_is_limited() {
    let output = CommandRunner::new()
        .max_stdout_bytes(1)
        .run(Command::shell("rustc --version && rustc --version 1>&2"))
        .expect("shell command should run successfully");

    assert_eq!(output.stdout().len(), 1);
    assert!(output.stdout_truncated());
    assert!(output.stderr().starts_with(b"rustc "));
}
