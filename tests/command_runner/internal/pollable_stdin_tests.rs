// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for cancellation-aware stdin polling.

#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use qubit_command::Command;
#[cfg(not(windows))]
use qubit_command::CommandRunner;

#[cfg(not(windows))]
#[test]
fn test_pollable_stdin_drains_large_input_without_blocking() {
    let input = vec![b'x'; 256 * 1024];
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("cat").stdin_bytes(input.clone()))
        .expect("large stdin should be delivered and the pipe should close");

    assert_eq!(output.stdout(), input);
    assert_eq!(output.exit_code(), Some(0));
}
