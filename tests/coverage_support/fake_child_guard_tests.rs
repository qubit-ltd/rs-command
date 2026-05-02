/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage-only fake child guard support.

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
    OutputStream,
};

#[test]
fn test_fake_child_guard_enables_synthetic_children_temporarily() {
    qubit_command::coverage_support::with_fake_children_enabled(|| {
        let error = CommandRunner::new()
            .run(Command::new("__qubit_command_missing_stdout__"))
            .expect_err("synthetic child should be enabled inside the guard");

        assert!(matches!(
            error,
            CommandError::ReadOutputFailed {
                stream: OutputStream::Stdout,
                ..
            },
        ));
    });

    let error = CommandRunner::new()
        .run(Command::new("__qubit_command_missing_stdout__"))
        .expect_err("synthetic child should be disabled after guard drop");
    assert!(matches!(error, CommandError::SpawnFailed { .. }));
}
