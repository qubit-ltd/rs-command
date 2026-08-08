// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for handing a started child and its I/O helpers to the runner.

use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandRunner;

#[test]
fn test_starting_command_hands_child_and_io_helpers_to_runner() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("rustc").arg("--version"))
        .expect("started command should transfer into the running state");

    assert!(output.stdout().starts_with(b"rustc "));
    assert!(output.stderr().is_empty());
}
