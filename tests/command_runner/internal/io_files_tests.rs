// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for conflict-safe command I/O file preparation.

use std::fs;
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command as ProcessCommand;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
#[cfg(target_os = "linux")]
use qubit_command::OutputStream;

use crate::support::LocalTempDir;

/// Creates a command that conflict tests must reject before spawning.
///
/// # Returns
///
/// A command whose executable intentionally does not exist.
fn unspawnable_command() -> Command {
    Command::new("__qubit_command_should_not_spawn__")
}

/// Creates a temporary directory for one test run.
///
/// # Returns
///
/// An armed temporary-directory guard.
fn temp_dir() -> LocalTempDir {
    LocalTempDir::with_prefix("qubit-command-io-files-")
        .expect("command I/O test temp directory should be created")
}

/// Creates a Unix FIFO at `path` for special-file rejection tests.
#[cfg(unix)]
fn create_fifo(path: &Path) {
    let status = ProcessCommand::new("mkfifo")
        .arg(path)
        .status()
        .expect("mkfifo should start");
    assert!(status.success(), "mkfifo should create the fixture");
}

#[test]
fn test_runner_rejects_stdin_stdout_conflict_without_truncating_input() {
    let temp_dir = temp_dir();
    let path = temp_dir.path().join("stdin-stdout-conflict");
    fs::write(&path, b"preserve-me").expect("stdin fixture should be written");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command().stdin_file(&path),
            CommandRunOptions::new().tee_stdout_to_file(&path),
        )
        .expect_err("conflicting files should be rejected");

    assert_eq!(error.kind(), CommandErrorKind::InputOutputConflict);
    assert_eq!(
        fs::read(&path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
}

#[test]
fn test_runner_rejects_stdin_stderr_conflict_without_truncating_input() {
    let temp_dir = temp_dir();
    let path = temp_dir.path().join("stdin-stderr-conflict");
    fs::write(&path, b"preserve-me").expect("stdin fixture should be written");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command().stdin_file(&path),
            CommandRunOptions::new().tee_stderr_to_file(&path),
        )
        .expect_err("conflicting files should be rejected");

    assert_eq!(error.kind(), CommandErrorKind::InputOutputConflict);
    assert_eq!(
        fs::read(&path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
}

#[test]
fn test_runner_rejects_stdout_stderr_conflict_before_creating_file() {
    let temp_dir = temp_dir();
    let path = temp_dir.path().join("stdout-stderr-conflict");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command(),
            CommandRunOptions::new()
                .tee_stdout_to_file(&path)
                .tee_stderr_to_file(&path),
        )
        .expect_err("conflicting output files should be rejected");

    assert_eq!(error.kind(), CommandErrorKind::OutputFilesConflict);
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn test_runner_rejects_symlinked_input_output_conflict() {
    let temp_dir = temp_dir();
    let input_path = temp_dir.path().join("symlink-conflict-input");
    let output_path = temp_dir.path().join("symlink-conflict-output");
    fs::write(&input_path, b"preserve-me")
        .expect("stdin fixture should be written");
    std::os::unix::fs::symlink(&input_path, &output_path)
        .expect("symlink fixture should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command().stdin_file(&input_path),
            CommandRunOptions::new().tee_stdout_to_file(&output_path),
        )
        .expect_err("symlinked files should be rejected");

    assert_eq!(error.kind(), CommandErrorKind::InputOutputConflict);
    assert_eq!(
        fs::read(&input_path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
}

#[test]
fn test_runner_rejects_hard_linked_input_output_conflict() {
    let temp_dir = temp_dir();
    let input_path = temp_dir.path().join("hard-link-conflict-input");
    let output_path = temp_dir.path().join("hard-link-conflict-output");
    fs::write(&input_path, b"preserve-me")
        .expect("stdin fixture should be written");
    fs::hard_link(&input_path, &output_path)
        .expect("hard link fixture should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command().stdin_file(&input_path),
            CommandRunOptions::new().tee_stdout_to_file(&output_path),
        )
        .expect_err("hard-linked files should be rejected");

    assert_eq!(error.kind(), CommandErrorKind::InputOutputConflict);
    assert_eq!(
        fs::read(&input_path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
}

#[test]
fn test_runner_rejects_hard_linked_input_stderr_conflict() {
    let temp_dir = temp_dir();
    let input_path = temp_dir.path().join("hard-link-stderr-input");
    let output_path = temp_dir.path().join("hard-link-stderr-output");
    fs::write(&input_path, b"preserve-me")
        .expect("stdin fixture should be written");
    fs::hard_link(&input_path, &output_path)
        .expect("hard link fixture should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command().stdin_file(&input_path),
            CommandRunOptions::new().tee_stderr_to_file(&output_path),
        )
        .expect_err("hard-linked files should be rejected");

    assert_eq!(error.kind(), CommandErrorKind::InputOutputConflict);
    assert_eq!(
        fs::read(&input_path).expect("stdin fixture should remain readable"),
        b"preserve-me",
    );
}

#[test]
fn test_runner_rejects_hard_linked_output_files() {
    let temp_dir = temp_dir();
    let stdout_path = temp_dir.path().join("hard-link-stdout");
    let stderr_path = temp_dir.path().join("hard-link-stderr");
    fs::write(&stdout_path, b"preserve-me")
        .expect("stdout fixture should be written");
    fs::hard_link(&stdout_path, &stderr_path)
        .expect("hard link fixture should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command(),
            CommandRunOptions::new()
                .tee_stdout_to_file(&stdout_path)
                .tee_stderr_to_file(&stderr_path),
        )
        .expect_err("hard-linked output files should be rejected");

    assert_eq!(error.kind(), CommandErrorKind::OutputFilesConflict);
    assert_eq!(
        fs::read(&stdout_path).expect("fixture should remain readable"),
        b"preserve-me",
    );
}

#[cfg(not(windows))]
#[test]
fn test_runner_normalizes_relative_output_path_components() {
    let temp_dir = LocalTempDir::in_dir(
        ".",
        Some("qubit-command-relative-output-"),
        128,
    )
    .expect("relative output temp directory should be created");
    let dir_name = temp_dir
        .path()
        .file_name()
        .expect("temporary directory should have a file name")
        .to_owned();
    let path = PathBuf::from(".")
        .join(&dir_name)
        .join("..")
        .join(dir_name)
        .join("relative-output");

    let output = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::shell("printf relative"),
            CommandRunOptions::new().tee_stdout_to_file(&path),
        )
        .expect("normalized relative output should be accepted");

    assert_eq!(output.stdout(), b"relative");
    assert_eq!(
        fs::read(&path).expect("relative output should be readable"),
        b"relative",
    );
}

#[cfg(unix)]
#[test]
fn test_runner_reports_symlink_loop_during_path_inspection() {
    let temp_dir = temp_dir();
    let first = temp_dir.path().join("symlink-loop-first");
    let second = temp_dir.path().join("symlink-loop-second");
    std::os::unix::fs::symlink(&second, &first)
        .expect("first symlink should be created");
    std::os::unix::fs::symlink(&first, &second)
        .expect("second symlink should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::shell("printf ignored"),
            CommandRunOptions::new().tee_stdout_to_file(&first),
        )
        .expect_err("symlink loop should fail path inspection");

    assert_eq!(error.kind(), CommandErrorKind::InspectIoFileFailed);
}

#[cfg(target_os = "linux")]
#[test]
fn test_runner_rejects_output_device() {
    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::shell("printf ignored"),
            CommandRunOptions::new().tee_stdout_to_file("/dev/full"),
        )
        .expect_err("full output device should be rejected before spawn");

    assert!(matches!(
        error.reason(),
        CommandErrorReason::NonRegularOutputFile {
            stream: OutputStream::Stdout,
            ..
        }
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn test_runner_rejects_stderr_device() {
    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::shell("printf ignored >&2"),
            CommandRunOptions::new().tee_stderr_to_file("/dev/full"),
        )
        .expect_err("full output device should be rejected before spawn");

    assert!(matches!(
        error.reason(),
        CommandErrorReason::NonRegularOutputFile {
            stream: OutputStream::Stderr,
            ..
        }
    ));
}

#[cfg(unix)]
#[test]
fn test_runner_rejects_fifo_for_stdin_path() {
    let temp_dir = temp_dir();
    let path = temp_dir.path().join("stdin-fifo");
    create_fifo(&path);

    let error = CommandRunner::new(Duration::from_secs(10))
        .run(unspawnable_command().stdin_file(&path))
        .expect_err("FIFO stdin should be rejected before spawn");

    assert_eq!(error.kind(), CommandErrorKind::NonRegularInputFile);
}

#[cfg(unix)]
#[test]
fn test_runner_rejects_fifo_for_stdout_path() {
    let temp_dir = temp_dir();
    let path = temp_dir.path().join("stdout-fifo");
    create_fifo(&path);

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command(),
            CommandRunOptions::new().tee_stdout_to_file(&path),
        )
        .expect_err("FIFO stdout should be rejected before spawn");

    assert!(matches!(
        error.reason(),
        CommandErrorReason::NonRegularOutputFile {
            stream: OutputStream::Stdout,
            ..
        }
    ));
}

#[cfg(unix)]
#[test]
fn test_runner_rejects_fifo_for_stderr_path() {
    let temp_dir = temp_dir();
    let path = temp_dir.path().join("stderr-fifo");
    create_fifo(&path);

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            unspawnable_command(),
            CommandRunOptions::new().tee_stderr_to_file(&path),
        )
        .expect_err("FIFO stderr should be rejected before spawn");

    assert!(matches!(
        error.reason(),
        CommandErrorReason::NonRegularOutputFile {
            stream: OutputStream::Stderr,
            ..
        }
    ));
}

#[cfg(unix)]
#[test]
fn test_runner_accepts_regular_files_after_handle_validation() {
    let temp_dir = temp_dir();
    let input_path = temp_dir.path().join("regular-input");
    let output_path = temp_dir.path().join("regular-output");
    fs::write(&input_path, b"regular-input")
        .expect("regular stdin fixture should be written");

    let output = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::new("cat").stdin_file(&input_path),
            CommandRunOptions::new().tee_stdout_to_file(&output_path),
        )
        .expect("regular files should remain usable");

    assert_eq!(output.stdout(), b"regular-input");
    assert_eq!(
        fs::read(&output_path).expect("regular tee output should be readable"),
        b"regular-input",
    );
}
