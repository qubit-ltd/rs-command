/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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

    assert_eq!(output.stdout_bytes(), b"writer-input");
}
