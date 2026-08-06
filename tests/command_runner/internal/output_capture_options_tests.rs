// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for output capture options.

use std::fs;

use crate::support::LocalTempDir;
use qubit_command::{
    Command,
    CommandRunOptions,
    CommandRunner,
};

#[test]
fn test_output_capture_options_keep_full_tee_with_limited_memory() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-output-capture-")
        .expect("output capture temp directory should be created");
    let path = temp_dir.path().join("stdout-capture-options.txt");
    let output = CommandRunner::new(Duration::from_secs(10))
        .max_stdout_bytes(5)
        .fail_on_output_truncation(false)
        .run_with(
            Command::new("rustc").arg("--version"),
            CommandRunOptions::new().tee_stdout_to_file(path.clone()),
        )
        .expect("rustc version command should run successfully");

    let file_bytes = fs::read(&path).expect("tee file should be readable");
    assert_eq!(output.stdout().len(), 5);
    assert!(file_bytes.starts_with(b"rustc "));
    assert!(file_bytes.len() > output.stdout().len());
}
