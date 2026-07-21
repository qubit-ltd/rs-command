// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for command runner error mapping.

#[cfg(not(windows))]
use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};
#[cfg(not(windows))]
use qubit_local_files::LocalTempDir;

#[cfg(not(windows))]
#[test]
fn test_error_mapping_preserves_unexpected_exit_output() {
    let error = CommandRunner::new()
        .run(Command::shell(
            "printf mapped-out; printf mapped-err >&2; exit 9",
        ))
        .expect_err("non-success exit should be mapped");

    match error {
        CommandError::UnexpectedExit {
            exit_code, output, ..
        } => {
            assert_eq!(exit_code, Some(9));
            assert_eq!(output.stdout(), b"mapped-out");
            assert_eq!(output.stderr(), b"mapped-err");
        }
        other => panic!("expected unexpected-exit error, got {other:?}"),
    }
}

#[cfg(not(windows))]
#[test]
fn test_error_mapping_redacts_io_paths_in_diagnostics() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-error-mapping-")
        .expect("error mapping temp directory should be created");
    let path = temp_dir.path().join("private-input");
    let error = CommandRunner::new()
        .run(Command::new("cat").stdin_file(&path))
        .expect_err("missing private stdin file should fail");
    let path_text = path.to_string_lossy();

    assert!(matches!(
        error,
        CommandError::OpenInputFailed { ref path, .. } if path == path_text.as_ref()
    ));
    assert!(!error.to_string().contains(path_text.as_ref()));
    assert!(!format!("{error:?}").contains(path_text.as_ref()));
}
