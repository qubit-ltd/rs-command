// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for conflict-safe command I/O file preparation.

use std::{
    fs,
    path::PathBuf,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
    OutputStream,
};

fn unique_temp_path(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "qubit-command-{name}-{}-{suffix}",
        std::process::id(),
    ))
}

#[test]
fn test_runner_rejects_stdin_stdout_conflict_without_truncating_input() {
    let path = unique_temp_path("stdin-stdout-conflict");
    fs::write(&path, b"preserve-me").expect("stdin fixture should be written");

    let error = CommandRunner::new()
        .tee_stdout_to_file(&path)
        .run(Command::new("cat").stdin_file(&path))
        .expect_err("conflicting files should be rejected");

    assert!(matches!(error, CommandError::InputOutputConflict { .. }));
    assert_eq!(
        fs::read(&path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
    fs::remove_file(path).expect("stdin fixture should be removed");
}

#[test]
fn test_runner_rejects_stdin_stderr_conflict_without_truncating_input() {
    let path = unique_temp_path("stdin-stderr-conflict");
    fs::write(&path, b"preserve-me").expect("stdin fixture should be written");

    let error = CommandRunner::new()
        .tee_stderr_to_file(&path)
        .run(Command::new("cat").stdin_file(&path))
        .expect_err("conflicting files should be rejected");

    assert!(matches!(error, CommandError::InputOutputConflict { .. }));
    assert_eq!(
        fs::read(&path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
    fs::remove_file(path).expect("stdin fixture should be removed");
}

#[test]
fn test_runner_rejects_stdout_stderr_conflict_before_creating_file() {
    let path = unique_temp_path("stdout-stderr-conflict");

    let error = CommandRunner::new()
        .tee_stdout_to_file(&path)
        .tee_stderr_to_file(&path)
        .run(Command::shell("printf out; printf err >&2"))
        .expect_err("conflicting output files should be rejected");

    assert!(matches!(error, CommandError::OutputFilesConflict { .. }));
    assert!(!path.exists());
}

#[test]
fn test_runner_rejects_symlinked_input_output_conflict() {
    let input_path = unique_temp_path("symlink-conflict-input");
    let output_path = unique_temp_path("symlink-conflict-output");
    fs::write(&input_path, b"preserve-me")
        .expect("stdin fixture should be written");
    std::os::unix::fs::symlink(&input_path, &output_path)
        .expect("symlink fixture should be created");

    let error = CommandRunner::new()
        .tee_stdout_to_file(&output_path)
        .run(Command::new("cat").stdin_file(&input_path))
        .expect_err("symlinked files should be rejected");

    assert!(matches!(error, CommandError::InputOutputConflict { .. }));
    assert_eq!(
        fs::read(&input_path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
    fs::remove_file(output_path).expect("symlink should be removed");
    fs::remove_file(input_path).expect("stdin fixture should be removed");
}

#[test]
fn test_runner_rejects_hard_linked_input_output_conflict() {
    let input_path = unique_temp_path("hard-link-conflict-input");
    let output_path = unique_temp_path("hard-link-conflict-output");
    fs::write(&input_path, b"preserve-me")
        .expect("stdin fixture should be written");
    fs::hard_link(&input_path, &output_path)
        .expect("hard link fixture should be created");

    let error = CommandRunner::new()
        .tee_stdout_to_file(&output_path)
        .run(Command::new("cat").stdin_file(&input_path))
        .expect_err("hard-linked files should be rejected");

    assert!(matches!(error, CommandError::InputOutputConflict { .. }));
    assert_eq!(
        fs::read(&input_path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
    fs::remove_file(output_path).expect("hard link should be removed");
    fs::remove_file(input_path).expect("stdin fixture should be removed");
}

#[test]
fn test_runner_rejects_hard_linked_input_stderr_conflict() {
    let input_path = unique_temp_path("hard-link-stderr-input");
    let output_path = unique_temp_path("hard-link-stderr-output");
    fs::write(&input_path, b"preserve-me")
        .expect("stdin fixture should be written");
    fs::hard_link(&input_path, &output_path)
        .expect("hard link fixture should be created");

    let error = CommandRunner::new()
        .tee_stderr_to_file(&output_path)
        .run(Command::new("cat").stdin_file(&input_path))
        .expect_err("hard-linked files should be rejected");

    assert!(matches!(error, CommandError::InputOutputConflict { .. }));
    assert_eq!(
        fs::read(&input_path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
    fs::remove_file(output_path).expect("hard link should be removed");
    fs::remove_file(input_path).expect("stdin fixture should be removed");
}

#[test]
fn test_runner_rejects_hard_linked_output_files() {
    let stdout_path = unique_temp_path("hard-link-stdout");
    let stderr_path = unique_temp_path("hard-link-stderr");
    fs::write(&stdout_path, b"preserve-me")
        .expect("stdout fixture should be written");
    fs::hard_link(&stdout_path, &stderr_path)
        .expect("hard link fixture should be created");

    let error = CommandRunner::new()
        .tee_stdout_to_file(&stdout_path)
        .tee_stderr_to_file(&stderr_path)
        .run(Command::shell("printf out; printf err >&2"))
        .expect_err("hard-linked output files should be rejected");

    assert!(matches!(error, CommandError::OutputFilesConflict { .. }));
    assert_eq!(
        fs::read(&stdout_path).expect("fixture should remain readable"),
        b"preserve-me",
    );
    fs::remove_file(stderr_path).expect("hard link should be removed");
    fs::remove_file(stdout_path).expect("stdout fixture should be removed");
}

#[test]
fn test_runner_normalizes_relative_output_path_components() {
    let file_name = unique_temp_path("relative-output")
        .file_name()
        .expect("temporary path should have a file name")
        .to_owned();
    let path = PathBuf::from(".")
        .join("target")
        .join("..")
        .join("target")
        .join(file_name);

    let output = CommandRunner::new()
        .tee_stdout_to_file(&path)
        .run(Command::shell("printf relative"))
        .expect("normalized relative output should be accepted");

    assert_eq!(output.stdout(), b"relative");
    assert_eq!(
        fs::read(&path).expect("relative output should be readable"),
        b"relative",
    );
    fs::remove_file(path).expect("relative output should be removed");
}

#[test]
fn test_runner_reports_symlink_loop_during_path_inspection() {
    let first = unique_temp_path("symlink-loop-first");
    let second = unique_temp_path("symlink-loop-second");
    std::os::unix::fs::symlink(&second, &first)
        .expect("first symlink should be created");
    std::os::unix::fs::symlink(&first, &second)
        .expect("second symlink should be created");

    let error = CommandRunner::new()
        .tee_stdout_to_file(&first)
        .run(Command::shell("printf ignored"))
        .expect_err("symlink loop should fail path inspection");

    assert!(matches!(error, CommandError::InspectIoFileFailed { .. }));
    fs::remove_file(first).expect("first symlink should be removed");
    fs::remove_file(second).expect("second symlink should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_runner_reports_output_device_write_failure() {
    let error = CommandRunner::new()
        .tee_stdout_to_file("/dev/full")
        .run(Command::shell("printf ignored"))
        .expect_err("full output device should reject writes");

    assert!(matches!(
        error,
        CommandError::WriteOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn test_runner_reports_stderr_device_write_failure() {
    let error = CommandRunner::new()
        .tee_stderr_to_file("/dev/full")
        .run(Command::shell("printf ignored >&2"))
        .expect_err("full output device should reject stderr writes");

    assert!(matches!(
        error,
        CommandError::WriteOutputFailed {
            stream: OutputStream::Stderr,
            ..
        }
    ));
}
