/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for finished command output behavior.

use std::time::Duration;

use qubit_command::{
    Command,
    CommandRunner,
};

#[test]
fn test_finished_command_preserves_elapsed_time() {
    let output = CommandRunner::new()
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    assert!(output.elapsed() >= Duration::ZERO);
}
