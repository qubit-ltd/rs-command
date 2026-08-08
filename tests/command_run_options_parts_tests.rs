// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests the internal per-run option handoff through the public runner API.

use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;

#[test]
fn test_command_run_options_parts_reach_runner() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run_with(Command::shell("exit 0"), CommandRunOptions::new())
        .expect("run options should reach the command runner");

    assert_eq!(output.exit_code(), Some(0));
}
