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
    fs,
    path::PathBuf,
    sync::Once,
    time::{
        Duration,
        Instant,
        SystemTime,
        UNIX_EPOCH,
    },
};

#[cfg(coverage)]
use qubit_command::coverage_support;
use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
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
fn test_command_runner_default_configuration() {
    init_test_logger();
    let runner = CommandRunner::new();

    assert_eq!(runner.configured_timeout(), None);
    assert_eq!(runner.configured_success_exit_codes(), &[0]);
    assert!(runner.configured_working_directory().is_none());
    assert!(!runner.is_logging_disabled());
    assert!(!runner.is_lossy_output_enabled());
    assert_eq!(runner.configured_max_stdout_bytes(), None);
    assert_eq!(runner.configured_max_stderr_bytes(), None);
    assert!(runner.configured_stdout_file().is_none());
    assert!(runner.configured_stderr_file().is_none());
}

#[test]
fn test_command_runner_run_captures_stdout() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(Command::shell("printf command-out"))
        .expect("command should run successfully");

    assert_eq!(output.exit_code(), Some(0));
    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "command-out",
    );
    assert!(output.stderr_bytes().is_empty());
}

#[test]
fn test_command_runner_run_captures_stderr() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(Command::shell("printf command-error >&2"))
        .expect("command should run successfully");

    assert!(output.stdout_bytes().is_empty());
    assert_eq!(
        output.stderr().expect("stderr should be valid UTF-8"),
        "command-error",
    );
}

#[test]
fn test_command_runner_run_applies_environment_override() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(Command::shell("printf \"$QUBIT_COMMAND_TEST\"").env("QUBIT_COMMAND_TEST", "from-env"))
        .expect("command should receive environment override");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "from-env",
    );
}

#[test]
fn test_command_runner_run_applies_environment_remove() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(
            Command::shell("printf \"${QUBIT_COMMAND_TEST:-missing}\"")
                .env("QUBIT_COMMAND_TEST", "from-env")
                .env_remove("QUBIT_COMMAND_TEST"),
        )
        .expect("command should remove configured environment variable");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "missing",
    );
}

#[test]
fn test_command_runner_run_applies_environment_clear_then_set() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(
            Command::shell("printf \"$QUBIT_COMMAND_TEST\"")
                .env_clear()
                .env("QUBIT_COMMAND_TEST", "after-clear"),
        )
        .expect("command should run with cleared environment plus explicit set");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "after-clear",
    );
}

#[test]
fn test_command_runner_run_applies_working_directory_override() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(Command::shell("pwd").working_directory("/"))
        .expect("command should run in requested working directory");

    assert_eq!(
        output
            .stdout()
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
        .run(Command::shell("pwd"))
        .expect("command should run in runner working directory");

    assert_eq!(
        output
            .stdout()
            .expect("stdout should be valid UTF-8")
            .trim(),
        "/",
    );
}

#[test]
fn test_command_runner_run_reports_unexpected_exit() {
    init_test_logger();
    let error = CommandRunner::new()
        .run(Command::shell(
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
                output.stdout().expect("stdout should be valid UTF-8"),
                "fail-out",
            );
            assert_eq!(
                output.stderr().expect("stderr should be valid UTF-8"),
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
        .run(Command::shell("exit 7"))
        .expect("configured success exit code should be accepted");

    assert_eq!(output.exit_code(), Some(7));
}

#[test]
fn test_command_runner_run_accepts_configured_success_codes() {
    init_test_logger();
    let output = CommandRunner::new()
        .success_exit_codes(&[3, 7])
        .run(Command::shell("exit 3"))
        .expect("configured success exit code list should be accepted");

    assert_eq!(output.exit_code(), Some(3));
}

#[test]
fn test_command_runner_run_without_timeout() {
    init_test_logger();
    let output = CommandRunner::new()
        .without_timeout()
        .run(Command::shell("printf no-timeout"))
        .expect("command should run successfully without timeout");

    assert_eq!(output.exit_code(), Some(0));
    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "no-timeout",
    );
}

#[test]
fn test_command_runner_run_writes_stdin_bytes() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(Command::shell("cat").stdin_bytes(b"stdin-bytes".to_vec()))
        .expect("command should receive stdin bytes");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "stdin-bytes",
    );
}

#[test]
fn test_command_runner_run_ignores_stdin_broken_pipe_for_success() {
    init_test_logger();
    let input = vec![b'x'; 1024 * 1024];
    let output = CommandRunner::new()
        .run(Command::shell("true").stdin_bytes(input))
        .expect("closed stdin should not hide a successful exit");

    assert_eq!(output.exit_code(), Some(0));
}

#[test]
fn test_command_runner_run_preserves_exit_status_after_stdin_broken_pipe() {
    init_test_logger();
    let input = vec![b'x'; 1024 * 1024];
    let error = CommandRunner::new()
        .run(Command::shell("exit 7").stdin_bytes(input))
        .expect_err("non-success exit should remain visible after stdin closes");

    match error {
        CommandError::UnexpectedExit {
            exit_code,
            expected,
            ..
        } => {
            assert_eq!(exit_code, Some(7));
            assert_eq!(expected, vec![0]);
        }
        other => panic!("expected unexpected-exit error, got {other:?}"),
    }
}

#[test]
fn test_command_runner_run_reads_stdin_file() {
    init_test_logger();
    let path = unique_temp_path("stdin.txt");
    fs::write(&path, b"stdin-file").expect("stdin fixture should be written");

    let output = CommandRunner::new()
        .run(Command::shell("cat").stdin_file(path.clone()))
        .expect("command should receive stdin file");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "stdin-file",
    );
    let _ = fs::remove_file(path);
}

#[test]
fn test_command_runner_run_accepts_stdin_inherit() {
    init_test_logger();
    let output = CommandRunner::new()
        .run(Command::shell("printf inherited").stdin_inherit())
        .expect("command should run with inherited stdin");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "inherited",
    );
}

#[test]
fn test_command_runner_run_reports_missing_stdin_file() {
    init_test_logger();
    let path = unique_temp_path("missing-stdin.txt");
    let error = CommandRunner::new()
        .run(Command::shell("cat").stdin_file(path.clone()))
        .expect_err("missing stdin file should be reported");

    match error {
        CommandError::OpenInputFailed {
            path: actual_path, ..
        } => assert_eq!(actual_path, path),
        other => panic!("expected stdin open failure, got {other:?}"),
    }
}

#[test]
fn test_command_runner_disable_logging_updates_configuration() {
    let runner = CommandRunner::new().disable_logging(true);

    assert!(runner.is_logging_disabled());
}

#[test]
fn test_command_runner_lossy_output_updates_configuration() {
    let runner = CommandRunner::new().lossy_output(true);

    assert!(runner.is_lossy_output_enabled());
}

#[test]
fn test_command_runner_output_limit_updates_configuration() {
    let runner = CommandRunner::new().max_stdout_bytes(3).max_stderr_bytes(4);

    assert_eq!(runner.configured_max_stdout_bytes(), Some(3));
    assert_eq!(runner.configured_max_stderr_bytes(), Some(4));
}

#[test]
fn test_command_runner_output_file_updates_configuration() {
    let stdout_path = unique_temp_path("stdout-config.txt");
    let stderr_path = unique_temp_path("stderr-config.txt");
    let runner = CommandRunner::new()
        .tee_stdout_to_file(stdout_path.clone())
        .tee_stderr_to_file(stderr_path.clone());

    assert_eq!(runner.configured_stdout_file(), Some(stdout_path.as_path()));
    assert_eq!(runner.configured_stderr_file(), Some(stderr_path.as_path()));
}

#[test]
fn test_command_runner_run_suppresses_success_logging() {
    let output = CommandRunner::new()
        .disable_logging(true)
        .run(Command::shell("printf quiet-success"))
        .expect("command should run successfully when logging is disabled");

    assert_eq!(
        output.stdout().expect("stdout should be valid UTF-8"),
        "quiet-success",
    );
}

#[test]
fn test_command_runner_run_suppresses_failure_logging() {
    let error = CommandRunner::new()
        .disable_logging(true)
        .run(Command::shell("exit 8"))
        .expect_err("unexpected exit should still be reported when logging is disabled");

    assert!(matches!(error, CommandError::UnexpectedExit { .. }));
}

#[test]
fn test_command_runner_run_reports_timeout() {
    init_test_logger();
    let error = CommandRunner::new()
        .timeout(Duration::from_millis(50))
        .run(Command::shell("sleep 2"))
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
fn test_command_runner_run_kills_process_group_on_timeout() {
    init_test_logger();
    let start = Instant::now();
    let error = CommandRunner::new()
        .timeout(Duration::from_millis(50))
        .run(Command::shell("sleep 2 & wait"))
        .expect_err("process group should time out");

    assert!(matches!(error, CommandError::TimedOut { .. }));
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "timeout should not wait for a background child that inherited output pipes",
    );
}

#[test]
fn test_command_runner_run_limits_captured_output() {
    init_test_logger();
    let output = CommandRunner::new()
        .max_stdout_bytes(3)
        .max_stderr_bytes(2)
        .run(Command::shell("printf abcdef; printf wxyz >&2"))
        .expect("command should run successfully");

    assert_eq!(output.stdout_bytes(), b"abc");
    assert_eq!(output.stderr_bytes(), b"wx");
    assert!(output.stdout_truncated());
    assert!(output.stderr_truncated());
}

#[test]
fn test_command_runner_run_tees_output_to_files() {
    init_test_logger();
    let stdout_path = unique_temp_path("stdout.txt");
    let stderr_path = unique_temp_path("stderr.txt");

    let output = CommandRunner::new()
        .max_output_bytes(3)
        .tee_stdout_to_file(stdout_path.clone())
        .tee_stderr_to_file(stderr_path.clone())
        .run(Command::shell("printf abcdef; printf wxyz >&2"))
        .expect("command should run successfully");

    assert_eq!(output.stdout_bytes(), b"abc");
    assert_eq!(output.stderr_bytes(), b"wxy");
    assert_eq!(
        fs::read(&stdout_path).expect("stdout tee file should be readable"),
        b"abcdef",
    );
    assert_eq!(
        fs::read(&stderr_path).expect("stderr tee file should be readable"),
        b"wxyz",
    );

    let _ = fs::remove_file(stdout_path);
    let _ = fs::remove_file(stderr_path);
}

#[test]
fn test_command_runner_run_reports_output_file_open_failure() {
    init_test_logger();
    let path = unique_temp_path("missing-dir").join("stdout.txt");
    let error = CommandRunner::new()
        .tee_stdout_to_file(path.clone())
        .run(Command::shell("printf ignored"))
        .expect_err("missing output directory should be reported");

    match error {
        CommandError::OpenOutputFailed {
            stream,
            path: actual_path,
            ..
        } => {
            assert_eq!(stream, OutputStream::Stdout);
            assert_eq!(actual_path, path);
        }
        other => panic!("expected stdout open failure, got {other:?}"),
    }
}

#[test]
fn test_command_runner_run_reports_stderr_file_open_failure() {
    init_test_logger();
    let path = unique_temp_path("missing-dir").join("stderr.txt");
    let error = CommandRunner::new()
        .tee_stderr_to_file(path.clone())
        .run(Command::shell("printf ignored"))
        .expect_err("missing output directory should be reported");

    match error {
        CommandError::OpenOutputFailed {
            stream,
            path: actual_path,
            ..
        } => {
            assert_eq!(stream, OutputStream::Stderr);
            assert_eq!(actual_path, path);
        }
        other => panic!("expected stderr open failure, got {other:?}"),
    }
}

#[test]
fn test_command_runner_run_reports_spawn_failure() {
    init_test_logger();
    let error = CommandRunner::new()
        .run(Command::new("__qubit_command_missing_executable__"))
        .expect_err("missing executable should fail to spawn");

    assert!(matches!(error, CommandError::SpawnFailed { .. }));
}

#[test]
fn test_command_runner_error_uses_argv_style_command_text() {
    init_test_logger();
    let error = CommandRunner::new()
        .run(Command::new("__qubit_command_missing_executable__").arg("two words"))
        .expect_err("missing executable should fail to spawn");

    assert_eq!(
        error.command(),
        r#"["__qubit_command_missing_executable__", "two words"]"#,
    );
}

#[test]
#[cfg(coverage)]
fn test_command_runner_coverage_exercises_defensive_paths() {
    let diagnostics = coverage_support::exercise_defensive_paths();
    let disabled_fake = CommandRunner::new()
        .run(Command::new("__qubit_command_missing_stdout__"))
        .expect_err("synthetic child names should not be active outside the coverage guard");
    assert!(matches!(disabled_fake, CommandError::SpawnFailed { .. }));

    coverage_support::with_fake_children_enabled(|| {
        let missing_stdout = CommandRunner::new()
            .run(Command::new("__qubit_command_missing_stdout__"))
            .expect_err("missing synthetic stdout pipe should be reported");
        assert!(matches!(
            missing_stdout,
            CommandError::ReadOutputFailed {
                stream: OutputStream::Stdout,
                ..
            }
        ));

        let missing_stderr = CommandRunner::new()
            .run(Command::new("__qubit_command_missing_stderr__"))
            .expect_err("missing synthetic stderr pipe should be reported");
        assert!(matches!(
            missing_stderr,
            CommandError::ReadOutputFailed {
                stream: OutputStream::Stderr,
                ..
            }
        ));

        let try_wait_error = CommandRunner::new()
            .run(Command::new("__qubit_command_try_wait_error__"))
            .expect_err("synthetic try-wait failure should be reported");
        assert!(matches!(try_wait_error, CommandError::WaitFailed { .. }));
        let collected = coverage_support::take_collect_output_commands();
        assert!(
            collected
                .iter()
                .any(|command| command.contains("__qubit_command_try_wait_error__")),
            "try-wait cleanup should drain output helpers before returning",
        );

        let try_wait_cleanup_error = CommandRunner::new()
            .run(Command::new(
                "__qubit_command_try_wait_error_kill_cleanup__",
            ))
            .expect_err("synthetic try-wait cleanup fallback should preserve wait error");
        assert!(matches!(
            try_wait_cleanup_error,
            CommandError::WaitFailed { .. }
        ));
        let collected = coverage_support::take_collect_output_commands();
        assert!(
            collected
                .iter()
                .any(|command| command.contains("__qubit_command_try_wait_error_kill_cleanup__")),
            "try-wait cleanup fallback should drain output helpers when the child already exited",
        );

        let kill_error = CommandRunner::new()
            .timeout(Duration::ZERO)
            .run(Command::new("__qubit_command_kill_error__"))
            .expect_err("synthetic kill failure should be reported");
        assert!(matches!(kill_error, CommandError::KillFailed { .. }));
        let collected = coverage_support::take_collect_output_commands();
        assert!(
            collected
                .iter()
                .any(|command| command.contains("__qubit_command_kill_error__")),
            "kill-error cleanup should drain output helpers when the child already exited",
        );

        let wait_after_kill_error = CommandRunner::new()
            .timeout(Duration::ZERO)
            .run(Command::new("__qubit_command_wait_after_kill_error__"))
            .expect_err("synthetic wait-after-kill failure should be reported");
        assert!(matches!(
            wait_after_kill_error,
            CommandError::WaitFailed { .. }
        ));
        let collected = coverage_support::take_collect_output_commands();
        assert!(
            collected
                .iter()
                .any(|command| command.contains("__qubit_command_wait_after_kill_error__")),
            "wait-after-kill cleanup should drain output helpers when the child already exited",
        );

        let collect_output_error = CommandRunner::new()
            .run(Command::new("__qubit_command_collect_output_error__"))
            .expect_err("synthetic output collection failure should be reported");
        assert!(matches!(
            collect_output_error,
            CommandError::ReadOutputFailed {
                stream: OutputStream::Stdout,
                ..
            }
        ));

        let timeout_collect_output_error = CommandRunner::new()
            .timeout(Duration::ZERO)
            .run(Command::new(
                "__qubit_command_timeout_collect_output_error__",
            ))
            .expect_err("synthetic timeout output collection failure should be reported");
        assert!(matches!(
            timeout_collect_output_error,
            CommandError::ReadOutputFailed {
                stream: OutputStream::Stdout,
                ..
            }
        ));
    });

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
            .any(|message| message.contains("write failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("flush failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("stdin pipe was not created")),
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
            .any(|message| message.contains("collect stdin failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("reader failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("reader write failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("output reader thread panicked")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("stdin write failed")),
    );
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("stdin writer thread panicked")),
    );
}
