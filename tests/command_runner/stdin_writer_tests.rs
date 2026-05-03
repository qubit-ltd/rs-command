/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for stdin writer behavior.

#[cfg(not(windows))]
use qubit_command::{
    Command,
    CommandRunner,
};

#[cfg(not(windows))]
#[test]
fn test_stdin_writer_sends_configured_bytes() {
    let output = CommandRunner::new()
        .run(Command::new("cat").stdin_bytes(b"writer-input".to_vec()))
        .expect("stdin writer should send configured bytes");

    assert_eq!(output.stdout(), b"writer-input");
}
