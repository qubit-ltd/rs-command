/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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

    assert_eq!(output.stdout_bytes().len(), 1);
    assert!(output.stdout_truncated());
    assert!(output.stderr_bytes().starts_with(b"rustc "));
}
