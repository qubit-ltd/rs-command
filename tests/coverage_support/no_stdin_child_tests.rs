/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage-only synthetic child support.

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

#[test]
fn test_no_stdin_child_reports_missing_stdin_pipe() {
    let diagnostics = qubit_command::coverage_support::exercise_defensive_paths();

    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("stdin pipe was not created")),
    );

    qubit_command::coverage_support::with_fake_children_enabled(|| {
        let error = CommandRunner::new()
            .run(Command::new("__qubit_command_try_wait_error__"))
            .expect_err("synthetic child should report wait failure");

        assert!(matches!(error, CommandError::WaitFailed { .. }));
    });
}
