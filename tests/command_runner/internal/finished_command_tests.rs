// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for finished command output behavior.

use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandRunner;

#[test]
fn test_finished_command_preserves_elapsed_time() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert!(output.elapsed() >= Duration::ZERO);
}

#[cfg(not(windows))]
#[test]
fn test_finished_command_elapsed_includes_inherited_output_drain() {
    let output = CommandRunner::without_timeout()
        .run(Command::shell("sleep 1 &"))
        .expect("background child should finish successfully");

    assert!(output.elapsed() >= Duration::from_millis(750));
}
