/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for [`CommandRunner`](qubit_command::CommandRunner).

#![cfg(not(windows))]

use std::time::Duration;

use qubit_command::{
    CommandError,
    CommandRunner,
    CommandSpec,
    DEFAULT_COMMAND_TIMEOUT,
};

#[test]
fn test_command_runner_default_configuration() {
    let runner = CommandRunner::new();

    assert_eq!(runner.configured_timeout(), Some(DEFAULT_COMMAND_TIMEOUT));
    assert_eq!(runner.configured_success_exit_codes(), &[0]);
    assert!(runner.configured_working_directory().is_none());
    assert!(!runner.is_logging_disabled());
}

#[test]
fn test_command_runner_run_captures_stdout() {
    let output = CommandRunner::new()
        .run(CommandSpec::shell("printf command-out"))
        .expect("command should run successfully");

    assert_eq!(output.exit_code(), Some(0));
    assert_eq!(
        output.stdout_utf8().expect("stdout should be valid UTF-8"),
        "command-out",
    );
    assert!(output.stderr().is_empty());
}

#[test]
fn test_command_runner_run_captures_stderr() {
    let output = CommandRunner::new()
        .run(CommandSpec::shell("printf command-error >&2"))
        .expect("command should run successfully");

    assert!(output.stdout().is_empty());
    assert_eq!(
        output.stderr_utf8().expect("stderr should be valid UTF-8"),
        "command-error",
    );
}

#[test]
fn test_command_runner_run_applies_environment_override() {
    let output = CommandRunner::new()
        .run(
            CommandSpec::shell("printf \"$QUBIT_COMMAND_TEST\"")
                .env("QUBIT_COMMAND_TEST", "from-env"),
        )
        .expect("command should receive environment override");

    assert_eq!(
        output.stdout_utf8().expect("stdout should be valid UTF-8"),
        "from-env",
    );
}

#[test]
fn test_command_runner_run_applies_working_directory_override() {
    let output = CommandRunner::new()
        .run(CommandSpec::shell("pwd").working_directory("/"))
        .expect("command should run in requested working directory");

    assert_eq!(
        output
            .stdout_utf8()
            .expect("stdout should be valid UTF-8")
            .trim(),
        "/",
    );
}

#[test]
fn test_command_runner_run_reports_unexpected_exit() {
    let error = CommandRunner::new()
        .run(CommandSpec::shell(
            "printf fail-out; printf fail-err >&2; exit 7",
        ))
        .expect_err("non-success exit code should be rejected");

    match error {
        CommandError::UnexpectedExit {
            exit_code,
            expected,
            output,
            ..
        } => {
            assert_eq!(exit_code, Some(7));
            assert_eq!(expected, vec![0]);
            assert_eq!(
                output.stdout_utf8().expect("stdout should be valid UTF-8"),
                "fail-out",
            );
            assert_eq!(
                output.stderr_utf8().expect("stderr should be valid UTF-8"),
                "fail-err",
            );
        }
        other => panic!("expected unexpected-exit error, got {other:?}"),
    }
}

#[test]
fn test_command_runner_run_accepts_configured_success_code() {
    let output = CommandRunner::new()
        .success_exit_code(7)
        .run(CommandSpec::shell("exit 7"))
        .expect("configured success exit code should be accepted");

    assert_eq!(output.exit_code(), Some(7));
}

#[test]
fn test_command_runner_run_reports_timeout() {
    let error = CommandRunner::new()
        .timeout(Duration::from_millis(50))
        .run(CommandSpec::shell("sleep 2"))
        .expect_err("long-running command should time out");

    match error {
        CommandError::TimedOut {
            timeout, output, ..
        } => {
            assert_eq!(timeout, Duration::from_millis(50));
            assert!(output.elapsed() >= Duration::from_millis(50));
        }
        other => panic!("expected timeout error, got {other:?}"),
    }
}

#[test]
fn test_command_runner_run_reports_spawn_failure() {
    let error = CommandRunner::new()
        .run(CommandSpec::new("__qubit_command_missing_executable__"))
        .expect_err("missing executable should fail to spawn");

    assert!(matches!(error, CommandError::SpawnFailed { .. }));
}
