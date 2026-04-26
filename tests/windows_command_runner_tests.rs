/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Windows smoke tests for [`CommandRunner`](qubit_command::CommandRunner).

#![cfg(windows)]

use std::{
    fs,
    path::PathBuf,
    time::{
        Duration,
        SystemTime,
        UNIX_EPOCH,
    },
};

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

/// Removes trailing Windows line endings from command output.
///
/// # Parameters
///
/// * `text` - Captured output text.
///
/// # Returns
///
/// Output text without trailing CR/LF characters.
fn trim_windows_line_endings(text: &str) -> &str {
    text.trim_end_matches(['\r', '\n'])
}

/// Creates a unique temporary file path for one test run.
///
/// # Parameters
///
/// * `name` - Human-readable filename component.
///
/// # Returns
///
/// Path inside the platform temporary directory.
fn unique_temp_path(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "qubit-command-windows-{name}-{}-{suffix}",
        std::process::id(),
    ))
}

#[test]
fn test_windows_command_runner_captures_stdout() {
    let output = CommandRunner::new()
        .run(Command::shell("echo command-out"))
        .expect("Windows shell command should run successfully");

    assert_eq!(
        trim_windows_line_endings(output.stdout().expect("stdout should be UTF-8")),
        "command-out",
    );
}

#[test]
fn test_windows_command_runner_captures_stderr() {
    let output = CommandRunner::new()
        .run(Command::shell("echo command-error 1>&2"))
        .expect("Windows shell command should run successfully");

    assert_eq!(
        trim_windows_line_endings(output.stderr().expect("stderr should be UTF-8")),
        "command-error",
    );
}

#[test]
fn test_windows_command_runner_reports_timeout() {
    let error = CommandRunner::new()
        .timeout(Duration::from_millis(50))
        .run(Command::shell("ping -n 3 127.0.0.1 >NUL"))
        .expect_err("long-running Windows command should time out");

    assert!(matches!(error, CommandError::TimedOut { .. }));
}

#[test]
fn test_windows_command_runner_tees_output_to_file() {
    let stdout_path = unique_temp_path("stdout.txt");
    let output = CommandRunner::new()
        .max_stdout_bytes(3)
        .tee_stdout_to_file(stdout_path.clone())
        .run(Command::shell("echo abcdef"))
        .expect("Windows shell command should run successfully");

    assert_eq!(output.stdout_bytes(), b"abc");
    assert!(output.stdout_truncated());
    assert_eq!(
        trim_windows_line_endings(
            std::str::from_utf8(&fs::read(&stdout_path).expect("tee file should be readable"))
                .expect("tee file should contain UTF-8"),
        ),
        "abcdef",
    );

    let _ = fs::remove_file(stdout_path);
}
