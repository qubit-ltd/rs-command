// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandErrorReason`](qubit_command::CommandErrorReason).

use qubit_command::Command;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunner;

#[test]
fn test_command_error_reason_retains_unexpected_exit_details() {
    let error = CommandRunner::without_timeout()
        .run(Command::shell("exit 7"))
        .expect_err("non-zero exit should produce a reason");
    assert!(matches!(
        error.reason(),
        CommandErrorReason::UnexpectedExit {
            exit_code: Some(7),
            expected,
        } if expected == &[0]
    ));
}
