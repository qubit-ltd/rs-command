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

use std::{
    io,
    sync::Once,
    time::Duration,
};

#[cfg(coverage)]
use qubit_command::coverage_support;
use qubit_command::{
    CommandError,
    CommandRunner,
    CommandSpec,
    DEFAULT_COMMAND_TIMEOUT,
    OutputStream,
};

static LOGGER_INIT: Once = Once::new();
static TEST_LOGGER: TestLogger = TestLogger;

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, _record: &log::Record<'_>) {}

    fn flush(&self) {}
}

fn init_test_logger() {
    LOGGER_INIT.call_once(|| {
        log::set_logger(&TEST_LOGGER).expect("test logger should be installed once");
        log::set_max_level(log::LevelFilter::Trace);
    });
}

#[test]
fn test_command_runner_default_configuration() {
    init_test_logger();
    let runner = CommandRunner::new();

    assert_eq!(runner.configured_timeout(), Some(DEFAULT_COMMAND_TIMEOUT));
    assert_eq!(runner.configured_success_exit_codes(), &[0]);
    assert!(runner.configured_working_directory().is_none());
    assert!(!runner.is_logging_disabled());
}

#[test]
fn test_command_runner_run_captures_stdout() {
    init_test_logger();
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
    init_test_logger();
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
    init_test_logger();
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
    init_test_logger();
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
fn test_command_runner_run_applies_default_working_directory() {
    init_test_logger();
    let output = CommandRunner::new()
        .working_directory("/")
        .run(CommandSpec::shell("pwd"))
        .expect("command should run in runner working directory");

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
    init_test_logger();
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
    init_test_logger();
    let output = CommandRunner::new()
        .success_exit_code(7)
        .run(CommandSpec::shell("exit 7"))
        .expect("configured success exit code should be accepted");

    assert_eq!(output.exit_code(), Some(7));
}

#[test]
fn test_command_runner_run_accepts_configured_success_codes() {
    init_test_logger();
    let output = CommandRunner::new()
        .success_exit_codes(&[3, 7])
        .run(CommandSpec::shell("exit 3"))
        .expect("configured success exit code list should be accepted");

    assert_eq!(output.exit_code(), Some(3));
}

#[test]
fn test_command_runner_run_without_timeout() {
    init_test_logger();
    let output = CommandRunner::new()
        .without_timeout()
        .run(CommandSpec::shell("printf no-timeout"))
        .expect("command should run successfully without timeout");

    assert_eq!(output.exit_code(), Some(0));
    assert_eq!(
        output.stdout_utf8().expect("stdout should be valid UTF-8"),
        "no-timeout",
    );
}

#[test]
fn test_command_runner_disable_logging_updates_configuration() {
    let runner = CommandRunner::new().disable_logging(true);

    assert!(runner.is_logging_disabled());
}

#[test]
fn test_command_runner_run_suppresses_success_logging() {
    let output = CommandRunner::new()
        .disable_logging(true)
        .run(CommandSpec::shell("printf quiet-success"))
        .expect("command should run successfully when logging is disabled");

    assert_eq!(
        output.stdout_utf8().expect("stdout should be valid UTF-8"),
        "quiet-success",
    );
}

#[test]
fn test_command_runner_run_suppresses_failure_logging() {
    let error = CommandRunner::new()
        .disable_logging(true)
        .run(CommandSpec::shell("exit 8"))
        .expect_err("unexpected exit should still be reported when logging is disabled");

    assert!(matches!(error, CommandError::UnexpectedExit { .. }));
}

#[test]
fn test_command_runner_run_reports_timeout() {
    init_test_logger();
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
    init_test_logger();
    let error = CommandRunner::new()
        .run(CommandSpec::new("__qubit_command_missing_executable__"))
        .expect_err("missing executable should fail to spawn");

    assert!(matches!(error, CommandError::SpawnFailed { .. }));
}

#[test]
fn test_command_output_lossy_methods_replace_invalid_utf8() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(CommandSpec::shell("printf '\\377'; printf '\\377' >&2"))
        .expect("command should run successfully");

    assert!(output.stdout_utf8().is_err());
    assert!(output.stderr_utf8().is_err());
    assert_eq!(output.stdout_lossy(), "\u{fffd}");
    assert_eq!(output.stderr_lossy(), "\u{fffd}");
}

#[test]
fn test_output_stream_formats_name() {
    assert_eq!(OutputStream::Stdout.as_str(), "stdout");
    assert_eq!(OutputStream::Stderr.as_str(), "stderr");
    assert_eq!(OutputStream::Stdout.to_string(), "stdout");
    assert_eq!(OutputStream::Stderr.to_string(), "stderr");
}

#[test]
fn test_command_error_accessors_for_errors_without_output() {
    let spawn = CommandError::SpawnFailed {
        command: "missing".to_owned(),
        source: io::Error::new(io::ErrorKind::NotFound, "missing"),
    };
    assert_eq!(spawn.command(), "missing");
    assert!(spawn.output().is_none());
    assert!(spawn.to_string().contains("failed to spawn command"));

    let wait = CommandError::WaitFailed {
        command: "wait".to_owned(),
        source: io::Error::other("wait failed"),
    };
    assert_eq!(wait.command(), "wait");
    assert!(wait.output().is_none());
    assert!(wait.to_string().contains("failed to wait"));

    let kill = CommandError::KillFailed {
        command: "kill".to_owned(),
        timeout: Duration::from_secs(1),
        source: io::Error::other("kill failed"),
    };
    assert_eq!(kill.command(), "kill");
    assert!(kill.output().is_none());
    assert!(kill.to_string().contains("failed to kill"));

    let read = CommandError::ReadOutputFailed {
        command: "read".to_owned(),
        stream: OutputStream::Stdout,
        source: io::Error::other("read failed"),
    };
    assert_eq!(read.command(), "read");
    assert!(read.output().is_none());
    assert!(read.to_string().contains("failed to read stdout"));
}

#[test]
fn test_command_error_accessors_for_errors_with_output() {
    init_test_logger();
    let unexpected = CommandRunner::new()
        .run(CommandSpec::shell("printf output; exit 9"))
        .expect_err("non-success exit code should be rejected");
    assert!(unexpected.command().contains("exit 9"));
    assert_eq!(
        unexpected
            .output()
            .expect("unexpected exit should expose output")
            .stdout_utf8()
            .expect("stdout should be valid UTF-8"),
        "output",
    );

    let timed_out = CommandRunner::new()
        .timeout(Duration::from_millis(50))
        .run(CommandSpec::shell("printf before-timeout; sleep 2"))
        .expect_err("long-running command should time out");
    assert!(timed_out.command().contains("sleep 2"));
    assert_eq!(
        timed_out
            .output()
            .expect("timeout should expose captured output")
            .stdout_utf8()
            .expect("stdout should be valid UTF-8"),
        "before-timeout",
    );
}

#[test]
#[cfg(coverage)]
fn test_command_runner_coverage_exercises_defensive_paths() {
    let diagnostics = coverage_support::exercise_defensive_paths();

    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("failed to spawn command `spawn`")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("failed to wait for command `wait`")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("failed to kill timed-out command `kill`")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("failed to read stdout for command `pipe`")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("failed to read stderr for command `pipe`")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("read failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("collect stdout failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("collect stderr failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("reader failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("output reader thread panicked")),
    );
}
