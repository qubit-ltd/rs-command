// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for output capture error mapping.

use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
use qubit_command::OutputStream;

use crate::support::LocalTempDir;

#[test]
fn test_output_capture_error_reports_unopenable_stdout_file() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-output-error-")
        .expect("output error temp directory should be created");
    let missing_directory = temp_dir.path().join("missing-output-directory");
    let path = missing_directory.join("stdout.txt");
    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::new("rustc").arg("--version"),
            CommandRunOptions::new().tee_stdout_to_file(path),
        )
        .expect_err("missing output directory should be reported");

    assert_eq!(error.kind(), CommandErrorKind::OpenOutputFailed);
    assert!(matches!(
        error.reason(),
        CommandErrorReason::OpenOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));
}
