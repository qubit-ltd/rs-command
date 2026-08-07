// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for stdin pipe behavior.

#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use qubit_command::{
    Command,
    CommandRunner,
};

#[cfg(not(windows))]
#[test]
fn test_stdin_pipe_closes_after_configured_bytes() {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("cat").stdin_bytes(b"pipe-input".to_vec()))
        .expect("stdin bytes should be delivered and pipe should close");

    assert_eq!(output.stdout(), b"pipe-input");
    assert_eq!(output.exit_code(), Some(0));
}
