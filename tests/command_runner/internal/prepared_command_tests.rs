// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for prepared command behavior.

#[cfg(not(windows))]
use std::fs;
#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use qubit_command::Command;
#[cfg(not(windows))]
use qubit_command::CommandRunner;

#[cfg(not(windows))]
#[test]
fn test_prepared_command_applies_working_directory_override() {
    let working_directory =
        fs::canonicalize("/tmp").expect("/tmp should resolve");
    let output = CommandRunner::new(Duration::from_secs(10))
        .working_directory("/")
        .run(Command::shell("pwd").working_directory(&working_directory))
        .expect("command should run in per-command working directory");

    assert_eq!(
        output
            .stdout_text()
            .expect("pwd output should be valid UTF-8")
            .trim(),
        working_directory.to_string_lossy(),
    );
}
