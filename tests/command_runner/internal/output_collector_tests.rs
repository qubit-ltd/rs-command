// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for output collector behavior.

#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use qubit_command::Command;
#[cfg(not(windows))]
use qubit_command::CommandRunner;

#[cfg(not(windows))]
#[test]
fn test_output_collector_keeps_streams_separate() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::shell("printf stdout-bytes; printf stderr-bytes >&2"))
        .expect("command should run successfully");

    assert_eq!(output.stdout(), b"stdout-bytes");
    assert_eq!(output.stderr(), b"stderr-bytes");
}
