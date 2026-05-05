/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for command runner error mapping.

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

#[test]
#[cfg(not(windows))]
fn test_error_mapping_preserves_unexpected_exit_output() {
    let error = CommandRunner::new()
        .run(Command::shell(
            "printf mapped-out; printf mapped-err >&2; exit 9",
        ))
        .expect_err("non-success exit should be mapped");

    match error {
        CommandError::UnexpectedExit {
            exit_code, output, ..
        } => {
            assert_eq!(exit_code, Some(9));
            assert_eq!(output.stdout(), b"mapped-out");
            assert_eq!(output.stderr(), b"mapped-err");
        }
        other => panic!("expected unexpected-exit error, got {other:?}"),
    }
}
