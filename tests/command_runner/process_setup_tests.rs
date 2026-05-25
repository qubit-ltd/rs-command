/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for process setup behavior.

#[cfg(not(windows))]
use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

#[cfg(not(windows))]
#[test]
fn test_process_setup_reports_missing_stdin_file_before_spawn() {
    let missing = std::env::temp_dir().join(format!("qubit-command-missing-stdin-{}", std::process::id(),));

    let error = CommandRunner::new()
        .run(Command::new("cat").stdin_file(missing.clone()))
        .expect_err("missing stdin file should be reported");

    match error {
        CommandError::OpenInputFailed { path, .. } => assert_eq!(path, missing),
        other => panic!("expected input-open failure, got {other:?}"),
    }
}
