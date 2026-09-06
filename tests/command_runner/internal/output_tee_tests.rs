// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for output tee behavior.

use std::fs;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;

use crate::support::LocalTempDir;

#[test]
fn test_output_tee_streams_stderr_to_file() {
    let temp_dir =
        LocalTempDir::with_prefix("qubit-command-output-tee-").expect("output tee temp directory should be created");
    let path = temp_dir.path().join("stderr-tee.txt");
    let output = CommandRunner::new(Duration::from_secs(10))
        .max_stderr_bytes(5)
        .fail_on_output_truncation(false)
        .run_with(
            Command::shell("rustc --version 1>&2"),
            CommandRunOptions::new().tee_stderr_to_file(path.clone()),
        )
        .expect("shell command should run successfully");

    let file_bytes = fs::read(&path).expect("tee file should be readable");
    assert_eq!(output.stderr().len(), 5);
    assert!(output.stderr_truncated());
    assert!(file_bytes.starts_with(b"rustc "));
}
