/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for running command behavior.

use std::time::Duration;

use qubit_command::{
    Command,
    CommandRunner,
};

#[test]
fn test_running_command_completes_before_timeout() {
    let output = CommandRunner::new()
        .timeout(Duration::from_secs(5))
        .run(Command::new("rustc").arg("--version"))
        .expect("short command should finish before timeout");

    assert!(output.stdout().starts_with(b"rustc "));
}
