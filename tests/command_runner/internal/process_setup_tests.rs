// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for process setup behavior.

#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use qubit_command::Command;
#[cfg(not(windows))]
use qubit_command::CommandErrorReason;
#[cfg(not(windows))]
use qubit_command::CommandRunner;

#[cfg(not(windows))]
use crate::support::LocalTempDir;

#[cfg(not(windows))]
#[test]
fn test_process_setup_reports_missing_stdin_file_before_spawn() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-process-setup-")
        .expect("process setup temp directory should be created");
    let missing = temp_dir.path().join("missing-stdin");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("cat").stdin_file(missing.clone()))
        .expect_err("missing stdin file should be reported");

    match error.reason() {
        CommandErrorReason::OpenInputFailed { path, .. } => {
            assert_eq!(path, &missing)
        }
        other => panic!("expected input-open failure, got {other:?}"),
    }
}
