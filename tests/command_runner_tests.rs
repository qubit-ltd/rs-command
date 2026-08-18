// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandRunner`](qubit_command::CommandRunner).

#[cfg(not(windows))]
use std::ffi::OsStr;
use std::fs;
use std::time::Duration;
use std::time::Instant;

use qubit_command::Command;
use qubit_command::CommandCancellation;
use qubit_command::CommandErrorKind;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
#[cfg(not(windows))]
use qubit_command::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM;
#[cfg(not(windows))]
use qubit_command::OutputStream;
#[cfg(not(windows))]
use qubit_redact::InputOutputLimit;
#[cfg(not(windows))]
use qubit_redact::RedactionCompletion;
#[cfg(not(windows))]
use qubit_redact::RedactionPolicy;
#[cfg(not(windows))]
use qubit_redact::Redactor;
#[cfg(not(windows))]
use qubit_redact::Sensitivity;
#[cfg(not(windows))]
use qubit_redact::formats::argv::ArgvItem;

mod command_runner;
mod support;
use support::LocalTempDir;

#[test]
fn test_runner_pre_cancelled_command_does_not_prepare_output_file() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
        .expect("command test temporary directory should be created");
    let stdout_path = temp_dir.path().join("stdout.log");
    let cancellation = CommandCancellation::new();
    cancellation.cancel();

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::new("__qubit_command_must_not_start__"),
            CommandRunOptions::new()
                .cancellation(cancellation)
                .tee_stdout_to_file(&stdout_path),
        )
        .expect_err("pre-cancelled command should not be prepared or started");

    assert_eq!(error.kind(), CommandErrorKind::CancelledBeforeStart);
    assert!(!stdout_path.exists());
}

#[test]
fn test_runner_pre_cancelled_command_does_not_truncate_existing_output_file() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
        .expect("command test temporary directory should be created");
    let stdout_path = temp_dir.path().join("stdout.log");
    fs::write(&stdout_path, b"must-survive-cancellation")
        .expect("tee fixture should be written");
    let cancellation = CommandCancellation::new();
    cancellation.cancel();

    let error = CommandRunner::without_timeout()
        .run_with(
            Command::new("__qubit_command_must_not_start__"),
            CommandRunOptions::new()
                .cancellation(cancellation)
                .tee_stdout_to_file(&stdout_path),
        )
        .expect_err("pre-cancelled command should not commit tee I/O");

    assert_eq!(error.kind(), CommandErrorKind::CancelledBeforeStart);
    assert_eq!(
        fs::read(&stdout_path).expect("tee fixture should remain readable"),
        b"must-survive-cancellation"
    );
}

#[test]
fn test_command_runner_rejects_directory_as_stdin_file() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
        .expect("command test temporary directory should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("true").stdin_file(temp_dir.path()))
        .expect_err("a directory must not be used as command stdin");

    assert_eq!(error.kind(), CommandErrorKind::NonRegularInputFile);
}

#[test]
fn test_command_runner_rejects_directory_as_stdout_tee() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
        .expect("command test temporary directory should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::new("true"),
            CommandRunOptions::new().tee_stdout_to_file(temp_dir.path()),
        )
        .expect_err("a directory must not be used as stdout tee");

    assert!(matches!(
        error.reason(),
        CommandErrorReason::NonRegularOutputFile {
            stream: OutputStream::Stdout,
            ..
        }
    ));
}

#[test]
fn test_command_runner_rejects_directory_as_stderr_tee() {
    let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
        .expect("command test temporary directory should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run_with(
            Command::new("true"),
            CommandRunOptions::new().tee_stderr_to_file(temp_dir.path()),
        )
        .expect_err("a directory must not be used as stderr tee");

    assert!(matches!(
        error.reason(),
        CommandErrorReason::NonRegularOutputFile {
            stream: OutputStream::Stderr,
            ..
        }
    ));
}

#[cfg(not(windows))]
mod unix {
    use super::ArgvItem;
    use super::Command;
    use super::CommandCancellation;
    use super::CommandErrorKind;
    use super::CommandErrorReason;
    use super::CommandRunOptions;
    use super::CommandRunner;
    use super::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM;
    use super::Duration;
    use super::InputOutputLimit;
    use super::Instant;
    use super::LocalTempDir;
    use super::OsStr;
    use super::OutputStream;
    use super::RedactionCompletion;
    use super::RedactionPolicy;
    use super::Redactor;
    use super::Sensitivity;
    use super::fs;
    use super::support::captured_log_records_containing;
    use super::support::initialize_captured_logger;

    #[test]
    fn test_command_runner_default_configuration() {
        let runner = CommandRunner::new(Duration::from_secs(10));
        let tee_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command tee temporary directory should be created");
        let stdout_tee = tee_dir.path().join("stdout.log");
        let stderr_tee = tee_dir.path().join("stderr.log");

        assert_eq!(runner.configured_timeout(), Some(Duration::from_secs(10)),);
        assert_eq!(runner.configured_success_exit_codes(), &[0]);
        assert!(runner.configured_working_directory().is_none());
        assert!(!runner.is_logging_disabled());
        assert!(runner.is_output_truncation_failure_enabled());
        assert_eq!(
            runner.configured_max_stdout_bytes(),
            Some(DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
        );
        assert_eq!(
            runner.configured_max_stderr_bytes(),
            Some(DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
        );
        let output = runner
            .run_with(
                Command::new("true"),
                CommandRunOptions::new()
                    .tee_stdout_to_file(&stdout_tee)
                    .tee_stderr_to_file(&stderr_tee),
            )
            .expect("runner option helper should not be persisted");
        let _ = output;
        assert!(stdout_tee.exists());
        assert!(stderr_tee.exists());
    }

    #[test]
    fn test_command_runner_default_rejects_output_beyond_safe_limit() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("head -c 1048577 /dev/zero"))
            .expect_err("default runner should reject output beyond 1 MiB");

        assert_eq!(error.kind(), CommandErrorKind::OutputTruncated);
        let output = error
            .output()
            .expect("output truncation error should retain bounded output");
        assert_eq!(output.stdout().len(), DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM,);
        assert!(output.stdout_truncated());
    }

    #[test]
    fn test_command_runner_unbounded_output_disables_safe_limit() {
        let runner =
            CommandRunner::new(Duration::from_secs(10)).unbounded_output();

        assert_eq!(runner.configured_max_stdout_bytes(), None);
        assert_eq!(runner.configured_max_stderr_bytes(), None);
        assert!(!runner.is_output_truncation_failure_enabled());

        let output = runner
            .run(Command::shell("head -c 1048577 /dev/zero"))
            .expect(
                "explicitly unbounded output should retain the full stream",
            );
        assert_eq!(
            output.stdout().len(),
            DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM + 1,
        );
        assert!(!output.stdout_truncated());
    }

    #[test]
    fn test_runner_accepts_a_complete_diagnostic_redaction_policy() {
        let mut builder = RedactionPolicy::default().to_builder();
        builder
            .edit_fields()
            .raise("tenant_option", Sensitivity::Secret)
            .expect("the test policy field must be valid")
            .allow_exact("username")
            .expect("the test policy field must be valid");
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let runner = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy.clone());

        assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy,);
    }

    #[test]
    fn test_command_runner_shares_configured_diagnostic_input_budget() {
        let budget = InputOutputLimit::builder()
            .max_input_bytes(4)
            .max_output_bytes(128)
            .build()
            .expect("the small diagnostic budget should be valid");
        let mut builder = RedactionPolicy::default().to_builder();
        builder.limits().diagnostic_event(budget);
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(Command::new("xxx").env("A", "B").env_remove("C"))
            .expect_err("the missing executable should fail to spawn");

        assert!(error.command().contains(r#"argv: ["xxx"]"#));
        assert!(error.command().contains("A=B"));
        assert!(error.command().contains(r#""C""#));
    }

    #[test]
    fn test_command_runner_applies_one_output_budget_to_full_diagnostic() {
        let budget = InputOutputLimit::builder()
            .max_input_bytes(512)
            .max_output_bytes(48)
            .build()
            .expect("the small diagnostic budget should be valid");
        let mut builder = RedactionPolicy::default().to_builder();
        builder.limits().diagnostic_event(budget);
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error =
            CommandRunner::new(Duration::from_secs(10))
                .diagnostic_redaction_policy(policy)
                .run(Command::new("xxx").env("VISIBLE", "value").arg(
                    "argument-that-forces-the-full-diagnostic-to-truncate",
                ))
                .expect_err("the missing executable should fail to spawn");

        assert!(error.command().contains("argument-that-forces"));
    }

    #[test]
    fn test_command_runner_maps_exhausted_environment_to_marker() {
        let budget = InputOutputLimit::builder()
            .max_input_bytes(256)
            .max_output_bytes(80)
            .build()
            .expect("the diagnostic budget should be valid");
        let mut builder = RedactionPolicy::default().to_builder();
        builder.limits().diagnostic_event(budget);
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let missing_program = "x".repeat(46);

        let redactor = Redactor::new(policy.clone());
        let mut session = redactor.session();
        let argv = session.argv_with_mut(|argv| {
            argv.redact_heuristically([ArgvItem::plain(OsStr::new(
                &missing_program,
            ))])
        });
        assert_eq!(argv.completion(), RedactionCompletion::Complete);
        let env = session.env_with_mut(|env| {
            env.redact_os_pairs([(OsStr::new("MODE"), OsStr::new("debug"))])
        });
        assert_eq!(env.completion(), RedactionCompletion::Complete);

        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(Command::new(&missing_program).env("MODE", "debug"))
            .expect_err("the missing executable should fail to spawn");

        assert!(error.command().contains("env:"));
    }

    #[test]
    fn test_command_runner_debug_describes_configuration() {
        let debug =
            format!("{:?}", CommandRunner::new(Duration::from_secs(10)));

        assert!(debug.contains("CommandRunner"));
        assert!(debug.contains("success_exit_codes: [0]"));
        assert!(debug.contains("timer: \"<dyn Timer>\""));
    }

    #[test]
    fn test_command_runner_debug_redacts_path_configuration() {
        let debug = format!(
            "{:?}",
            CommandRunner::new(Duration::from_secs(10))
                .working_directory("customer/working-directory"),
        );
        let options_debug = format!(
            "{:?}",
            CommandRunOptions::new()
                .tee_stdout_to_file("customer/stdout.log")
                .tee_stderr_to_file("customer/stderr.log"),
        );

        assert!(debug.contains("working_directory: Some(\"<redacted path>\")"));
        assert!(
            options_debug.contains("stdout_file: Some(\"<redacted path>\")")
        );
        assert!(
            options_debug.contains("stderr_file: Some(\"<redacted path>\")")
        );
        assert!(!debug.contains("customer/working-directory"));
        assert!(!options_debug.contains("customer/stdout.log"));
        assert!(!options_debug.contains("customer/stderr.log"));
    }

    #[test]
    fn test_command_runner_run_captures_stdout() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("printf command-out"))
            .expect("command should run successfully");

        assert_eq!(output.exit_code(), Some(0));
        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "command-out",
        );
        assert!(output.stderr().is_empty());
        assert!(output.stdout_complete());
        assert!(output.stderr_complete());
    }

    #[test]
    fn test_runner_without_timeout_uses_parent_process_group() {
        let output = CommandRunner::without_timeout()
            .run(Command::shell("ps -o pgid= -p $$; ps -o pgid= -p $PPID"))
            .expect("command without timeout should run successfully");
        let process_groups: Vec<i32> = output
            .stdout_text()
            .expect("process groups should be valid UTF-8")
            .split_whitespace()
            .map(|value| {
                value.parse().expect("process group should be numeric")
            })
            .collect();

        assert_eq!(process_groups.len(), 2);
        assert_eq!(
            process_groups[0], process_groups[1],
            "a command without timeout must remain in its parent's process group",
        );
    }

    #[test]
    fn test_runner_cancellation_without_timeout_terminates_running_command() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let started_path = temp_dir.path().join("started");
        let script = format!(
            "printf started; : > '{}'; while :; do sleep 1; done",
            started_path.display(),
        );
        let cancellation = CommandCancellation::new();
        let runner = CommandRunner::without_timeout();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let run_cancellation = cancellation.clone();
        let worker = std::thread::spawn(move || {
            let result = runner.run_with(
                Command::shell(&script),
                CommandRunOptions::new().cancellation(run_cancellation),
            );
            sender
                .send(result)
                .expect("test receiver should remain connected");
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        while !started_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            started_path.exists(),
            "command should start before cancellation"
        );

        cancellation.cancel();
        let error = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("cancelled runner should return promptly")
            .expect_err("cancelled command should return an error");
        worker.join().expect("runner thread should not panic");

        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        assert_eq!(
            error.output().expect("cancelled output").stdout(),
            b"started"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_wakes_silent_inherited_stdout_reader() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        let script = "setsid sh -c 'echo \"$$\" > \"$1\"; sleep 10' sh \"$1\" & printf started";
        let cancellation = CommandCancellation::new();
        let run_cancellation = cancellation.clone();
        let run_pid_path = pid_path.clone();
        let worker = std::thread::spawn(move || {
            CommandRunner::without_timeout().run_with(
                Command::new("sh")
                    .arg("-c")
                    .arg(script)
                    .arg("sh")
                    .arg_os(&run_pid_path),
                CommandRunOptions::new().cancellation(run_cancellation),
            )
        });

        std::thread::sleep(Duration::from_millis(100));
        cancellation.cancel();
        let error = worker
            .join()
            .expect("cancelled runner should not panic")
            .expect_err("inherited output should make cancellation observable");

        if let Ok(pid) = fs::read_to_string(&pid_path) {
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(pid.trim())
                .status();
        }
        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_wakes_blocked_stdin_writer() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-stdin-child.pid");
        let script = "setsid sh -c 'echo \"$$\" > \"$1\"; sleep 10' sh \"$1\" >/dev/null 2>&1 & wait";
        let cancellation = CommandCancellation::new();
        let run_cancellation = cancellation.clone();
        let run_pid_path = pid_path.clone();
        let worker = std::thread::spawn(move || {
            CommandRunner::without_timeout().run_with(
                Command::shell(script)
                    .arg_os(&run_pid_path)
                    .stdin_bytes(vec![b'x'; 4 * 1024 * 1024]),
                CommandRunOptions::new().cancellation(run_cancellation),
            )
        });

        std::thread::sleep(Duration::from_millis(100));
        cancellation.cancel();
        let error = worker
            .join()
            .expect("cancelled runner should not panic")
            .expect_err("blocked stdin should make cancellation observable");

        if let Ok(pid) = fs::read_to_string(&pid_path) {
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(pid.trim())
                .status();
        }
        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
    }

    #[test]
    fn test_command_runner_run_captures_stderr() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("printf command-error >&2"))
            .expect("command should run successfully");

        assert!(output.stdout().is_empty());
        assert_eq!(
            output.stderr_text().expect("stderr should be valid UTF-8"),
            "command-error",
        );
        assert!(output.stdout_complete());
        assert!(output.stderr_complete());
    }

    #[test]
    fn test_command_runner_run_applies_environment_override() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::shell("printf \"$QUBIT_COMMAND_TEST\"")
                    .env("QUBIT_COMMAND_TEST", "from-env"),
            )
            .expect("command should receive environment override");

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "from-env",
        );
    }

    #[test]
    fn test_command_runner_run_applies_environment_remove() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::shell("printf \"${QUBIT_COMMAND_TEST:-missing}\"")
                    .env("QUBIT_COMMAND_TEST", "from-env")
                    .env_remove("QUBIT_COMMAND_TEST"),
            )
            .expect("command should remove configured environment variable");

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "missing",
        );
    }

    #[test]
    fn test_command_runner_run_applies_environment_clear_then_set() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::shell("printf \"$QUBIT_COMMAND_TEST\"")
                    .env_clear()
                    .env("QUBIT_COMMAND_TEST", "after-clear"),
            )
            .expect(
                "command should run with cleared environment plus explicit set",
            );

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "after-clear",
        );
    }

    #[test]
    fn test_command_runner_run_applies_working_directory_override() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("pwd").working_directory("/"))
            .expect("command should run in requested working directory");

        assert_eq!(
            output
                .stdout_text()
                .expect("stdout should be valid UTF-8")
                .trim(),
            "/",
        );
    }

    #[test]
    fn test_command_runner_run_applies_default_working_directory() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .working_directory("/")
            .run(Command::shell("pwd"))
            .expect("command should run in runner working directory");

        assert_eq!(
            output
                .stdout_text()
                .expect("stdout should be valid UTF-8")
                .trim(),
            "/",
        );
    }

    #[test]
    fn test_command_runner_run_reports_unexpected_exit() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell(
                "printf fail-out; printf fail-err >&2; exit 7",
            ))
            .expect_err("non-success exit code should be rejected");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert_eq!(error.exit_code(), Some(7));
        assert!(matches!(
            error.reason(),
            CommandErrorReason::UnexpectedExit { expected, .. } if expected == &[0]
        ));
        let output = error.output().expect("unexpected output");
        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "fail-out"
        );
        assert_eq!(
            output.stderr_text().expect("stderr should be valid UTF-8"),
            "fail-err"
        );
    }

    #[test]
    fn test_command_runner_run_accepts_configured_success_code() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .success_exit_code(7)
            .run(Command::shell("exit 7"))
            .expect("configured success exit code should be accepted");

        assert_eq!(output.exit_code(), Some(7));
    }

    #[test]
    fn test_command_runner_run_accepts_configured_success_codes() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .success_exit_codes(&[3, 7])
            .run(Command::shell("exit 3"))
            .expect("configured success exit code list should be accepted");

        assert_eq!(output.exit_code(), Some(3));
    }

    #[test]
    fn test_command_runner_run_without_timeout() {
        let output = CommandRunner::without_timeout()
            .run(Command::shell("printf no-timeout"))
            .expect("command should run successfully without timeout");

        assert_eq!(output.exit_code(), Some(0));
        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "no-timeout",
        );
    }

    #[test]
    fn test_runner_without_timeout_does_not_wait_on_injected_timer() {
        use qubit_clock::ManualMonotonicClock;
        use qubit_clock::MonotonicClock;

        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::without_timeout().timer(clock.new_timer());
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = runner.run(Command::shell("sleep 0.05"));
            sender
                .send(result)
                .expect("test receiver should remain connected");
        });

        let first_result = receiver.recv_timeout(Duration::from_millis(250));
        let completed_without_advance = first_result.is_ok();
        let result = match first_result {
            Ok(result) => result,
            Err(_) => {
                clock
                    .advance(Duration::from_millis(10))
                    .expect("manual time should advance for cleanup");
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .expect("runner should finish after cleanup advance")
            }
        };
        worker.join().expect("runner thread should not panic");

        assert!(
            completed_without_advance,
            "a runner without timeout must block on the child, not its timer",
        );
        let output = result.expect("command should complete successfully");
        assert_eq!(Some(0), output.exit_code());
    }

    #[test]
    fn test_command_runner_run_writes_stdin_bytes() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("cat").stdin_bytes(b"stdin-bytes".to_vec()))
            .expect("command should receive stdin bytes");

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "stdin-bytes",
        );
    }

    #[test]
    fn test_command_runner_run_ignores_stdin_broken_pipe_for_success() {
        let input = vec![b'x'; 1024 * 1024];
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("true").stdin_bytes(input))
            .expect("closed stdin should not hide a successful exit");

        assert_eq!(output.exit_code(), Some(0));
    }

    #[test]
    fn test_command_runner_run_preserves_exit_status_after_stdin_broken_pipe() {
        let input = vec![b'x'; 1024 * 1024];
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("exit 7").stdin_bytes(input))
            .expect_err(
                "non-success exit should remain visible after stdin closes",
            );

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert_eq!(error.exit_code(), Some(7));
        assert!(matches!(
            error.reason(),
            CommandErrorReason::UnexpectedExit { expected, .. } if expected == &[0]
        ));
    }

    #[test]
    fn test_command_runner_run_reads_stdin_file() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let path = temp_dir.path().join("stdin.txt");
        fs::write(&path, b"stdin-file")
            .expect("stdin fixture should be written");

        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("cat").stdin_file(path.clone()))
            .expect("command should receive stdin file");

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "stdin-file",
        );
    }

    #[test]
    fn test_command_runner_run_accepts_stdin_inherit() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("printf inherited").stdin_inherit())
            .expect("command should run with inherited stdin");

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "inherited",
        );
    }

    #[test]
    fn test_command_runner_run_reports_missing_stdin_file() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let path = temp_dir.path().join("missing-stdin.txt");
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("cat").stdin_file(path.clone()))
            .expect_err("missing stdin file should be reported");

        match error.reason() {
            CommandErrorReason::OpenInputFailed {
                path: actual_path, ..
            } => {
                assert_eq!(actual_path, &path)
            }
            other => panic!("expected stdin open failure, got {other:?}"),
        }
    }

    #[test]
    fn test_command_runner_disable_logging_updates_configuration() {
        let runner =
            CommandRunner::new(Duration::from_secs(10)).disable_logging(true);

        assert!(runner.is_logging_disabled());
    }

    #[test]
    fn test_command_runner_output_limit_updates_configuration() {
        let runner = CommandRunner::new(Duration::from_secs(10))
            .max_stdout_bytes(3)
            .max_stderr_bytes(4);

        assert_eq!(runner.configured_max_stdout_bytes(), Some(3));
        assert_eq!(runner.configured_max_stderr_bytes(), Some(4));
    }

    #[test]
    fn test_command_runner_can_accept_output_truncation() {
        let runner = CommandRunner::new(Duration::from_secs(10))
            .fail_on_output_truncation(false);

        assert!(!runner.is_output_truncation_failure_enabled());
    }

    #[test]
    fn test_command_runner_output_file_updates_configuration() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let stdout_path = temp_dir.path().join("stdout-config.txt");
        let stderr_path = temp_dir.path().join("stderr-config.txt");
        let options = CommandRunOptions::new()
            .tee_stdout_to_file(stdout_path.clone())
            .tee_stderr_to_file(stderr_path.clone());

        assert_eq!(
            options.configured_stdout_file(),
            Some(stdout_path.as_path())
        );
        assert_eq!(
            options.configured_stderr_file(),
            Some(stderr_path.as_path())
        );
    }

    #[test]
    fn test_command_runner_run_logs_success_lifecycle_at_debug() {
        const MARKER: &str = "qubit-command-log-success-marker";
        initialize_captured_logger();

        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::new("printf").arg(MARKER))
            .expect("command should run successfully");

        assert_eq!(output.stdout(), MARKER.as_bytes());
        let records = captured_log_records_containing(MARKER);
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|(level, _)| *level == log::Level::Debug));
        assert!(
            records
                .iter()
                .any(|(_, message)| message.contains("Running"))
        );
        assert!(
            records
                .iter()
                .any(|(_, message)| message.contains("Finished"))
        );
    }

    #[test]
    fn test_command_runner_run_suppresses_success_logging() {
        const MARKER: &str = "qubit-command-log-quiet-success-marker";
        initialize_captured_logger();

        let output = CommandRunner::new(Duration::from_secs(10))
            .disable_logging(true)
            .run(Command::new("printf").arg(MARKER))
            .expect("command should run successfully when logging is disabled");

        assert_eq!(output.stdout(), MARKER.as_bytes());
        assert!(captured_log_records_containing(MARKER).is_empty());
    }

    #[test]
    fn test_command_runner_run_logs_unexpected_exit_without_error_level() {
        const MARKER: &str = "qubit-command-log-failure-marker";
        initialize_captured_logger();

        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::new("sh").arg("-c").arg("exit 8").arg(MARKER))
            .expect_err("unexpected exit should be reported");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        let records = captured_log_records_containing(MARKER);
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|(level, _)| *level == log::Level::Debug));
    }

    #[test]
    fn test_command_runner_run_suppresses_failure_logging() {
        const MARKER: &str = "qubit-command-log-quiet-failure-marker";
        initialize_captured_logger();

        let error = CommandRunner::new(Duration::from_secs(10))
            .disable_logging(true)
            .run(Command::new("sh").arg("-c").arg("exit 8").arg(MARKER))
            .expect_err("unexpected exit should still be reported when logging is disabled");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert!(captured_log_records_containing(MARKER).is_empty());
    }

    #[test]
    fn test_command_runner_run_logs_redacted_command_text() {
        const MARKER: &str = "qubit-command-log-redacted-marker";
        const SECRET: &str = "command-log-secret";
        initialize_captured_logger();

        let output = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::new("printf")
                    .arg("%s")
                    .arg(MARKER)
                    .arg("--password")
                    .arg(SECRET),
            )
            .expect("command should run successfully");

        assert_eq!(
            output.stdout(),
            format!("{MARKER}--password{SECRET}").as_bytes()
        );

        let records = captured_log_records_containing(MARKER);
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|(_, message)| !message.contains(SECRET)));
    }

    #[test]
    fn test_command_runner_run_reports_timeout() {
        let error = CommandRunner::new(Duration::from_millis(50))
            .run(Command::shell("sleep 2"))
            .expect_err("long-running command should time out");

        match error.reason() {
            CommandErrorReason::TimedOut { timeout } => {
                assert_eq!(*timeout, Duration::from_millis(50));
                assert!(
                    error.output().expect("timeout output").elapsed()
                        >= Duration::from_millis(50)
                );
            }
            other => panic!("expected timeout error, got {other:?}"),
        }
    }

    #[test]
    fn test_runner_zero_timeout_does_not_report_kill_failure_after_exit() {
        // Exercise the short-lived child/killpg race repeatedly because the
        // process-group error can arrive just before the child becomes
        // waitable.
        for _ in 0..10_000 {
            if let Err(error) =
                CommandRunner::new(Duration::ZERO).run(Command::new("true"))
            {
                assert_ne!(error.kind(), CommandErrorKind::KillFailed);
                assert_ne!(error.kind(), CommandErrorKind::UnexpectedExit);
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_returns_when_descendant_escapes_process_group()
     {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        let escaped_child =
            "setsid sh -c 'echo \"$$\" > \"$1\"; sleep 10' sh \"$1\" &";
        let started = Instant::now();

        let error = CommandRunner::new(Duration::from_millis(100))
            .run(
                Command::new("sh")
                    .arg("-c")
                    .arg(escaped_child)
                    .arg("sh")
                    .arg_os(&pid_path),
            )
            .expect_err("escaped descendant should keep the output pipe open");

        let pid_deadline = Instant::now() + Duration::from_secs(1);
        while !pid_path.exists() && Instant::now() < pid_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Ok(pid) = fs::read_to_string(&pid_path) {
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(pid.trim())
                .status();
        }

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        let output = error
            .output()
            .expect("timeout should retain captured output metadata");
        assert!(!output.stdout_complete());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timeout must not wait for an escaped descendant to close inherited output"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_cancels_incomplete_stream() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        let escaped_child = "setsid sh -c 'echo \"$$\" > \"$1\"; sleep 10' sh \"$1\" >/dev/null &";
        let started = Instant::now();

        let error = CommandRunner::new(Duration::from_millis(100))
            .run(
                Command::new("sh")
                    .arg("-c")
                    .arg(escaped_child)
                    .arg("sh")
                    .arg_os(&pid_path),
            )
            .expect_err("escaped stderr descendant should time out");

        if let Ok(pid) = fs::read_to_string(&pid_path) {
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(pid.trim())
                .status();
        }

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timeout must not wait for an escaped stderr descendant"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_cancels_blocked_stdin_writer() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-stdin-child.pid");
        let started = Instant::now();
        let error = CommandRunner::new(Duration::from_millis(100))
            .run(
                Command::shell(
                    "setsid sh -c 'echo \"$$\" > \"$1\"; sleep 10' sh \"$1\" >/dev/null 2>&1 & wait",
                )
                .arg_os(&pid_path)
                .stdin_bytes(vec![b'x'; 4 * 1024 * 1024]),
            )
            .expect_err("escaped stdin descendant should make the command time out");

        let pid_deadline = Instant::now() + Duration::from_secs(1);
        while !pid_path.exists() && Instant::now() < pid_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Ok(pid) = fs::read_to_string(&pid_path) {
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(pid.trim())
                .status();
        }

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout must cancel a blocked stdin writer"
        );
    }

    #[test]
    fn test_runner_timeout_uses_injected_manual_timer() {
        use qubit_clock::ManualMonotonicClock;
        use qubit_clock::MonotonicClock;

        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::new(Duration::from_secs(30))
            .timer(clock.new_timer());
        let worker =
            std::thread::spawn(move || runner.run(Command::shell("sleep 60")));

        assert!(clock.wait_for_waiters(1, Duration::from_secs(2)));
        clock
            .advance(Duration::from_secs(30))
            .expect("manual time should advance");
        let error = worker
            .join()
            .expect("runner thread should not panic")
            .expect_err("command should time out");
        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
    }

    #[test]
    fn test_runner_timeout_accepts_child_that_exits_before_deadline() {
        use qubit_clock::ManualMonotonicClock;
        use qubit_clock::MonotonicClock;

        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let signal_path = temp_dir.path().join("deadline-signal");
        let completion_path = temp_dir.path().join("deadline-completion");
        let script = "while [ ! -e \"$1\" ]; do sleep 0.01; done; : > \"$2\"";
        let clock = ManualMonotonicClock::new_shared();
        let timeout = Duration::from_secs(30);
        let runner = CommandRunner::new(timeout).timer(clock.new_timer());
        let child_signal_path = signal_path.clone();
        let child_completion_path = completion_path.clone();
        let worker = std::thread::spawn(move || {
            runner.run(
                Command::new("sh")
                    .arg("-c")
                    .arg(script)
                    .arg("sh")
                    .arg_os(&child_signal_path)
                    .arg_os(&child_completion_path),
            )
        });

        assert!(clock.wait_for_waiters(1, Duration::from_secs(2)));
        fs::write(&signal_path, b"release")
            .expect("signal file should release child command");
        let completed_before_deadline = Instant::now() + Duration::from_secs(2);
        while !completion_path.exists() {
            assert!(
                Instant::now() < completed_before_deadline,
                "child command should exit before the manual deadline advances",
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(50));
        clock.advance(timeout).expect("manual time should advance");

        let result = worker.join().expect("runner thread should not panic");
        let _ = result.expect(
            "a child that exits before the deadline should complete normally",
        );
    }

    #[test]
    fn test_command_runner_timer_updates_configuration() {
        use qubit_clock::ManualMonotonicClock;
        use qubit_clock::MonotonicClock;

        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::new(Duration::from_secs(10))
            .timer(clock.new_timer());

        assert_eq!(
            runner.configured_timer().clock().now().domain(),
            clock.now().domain(),
        );
    }

    #[test]
    fn test_command_runner_run_kills_process_group_on_timeout() {
        let start = Instant::now();
        let error = CommandRunner::new(Duration::from_millis(50))
            .run(Command::shell("sleep 2 & wait"))
            .expect_err("process group should time out");

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "timeout should not wait for a background child that inherited output pipes",
        );
    }

    #[test]
    fn test_command_runner_run_times_out_when_background_child_inherits_output()
    {
        let start = Instant::now();
        let error = CommandRunner::new(Duration::from_millis(50))
            .run(Command::shell("sleep 2 &"))
            .expect_err(
                "background child with inherited output pipes should time out",
            );

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "timeout should include output collection after the direct child exits",
        );
    }

    #[test]
    fn test_command_runner_run_limits_captured_output() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .max_stdout_bytes(3)
            .max_stderr_bytes(2)
            .fail_on_output_truncation(false)
            .run(Command::shell("printf abcdef; printf wxyz >&2"))
            .expect("command should run successfully");

        assert_eq!(output.stdout(), b"abc");
        assert_eq!(output.stderr(), b"wx");
        assert!(output.stdout_truncated());
        assert!(output.stderr_truncated());
    }

    #[test]
    fn test_command_runner_run_fails_when_output_is_truncated() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .max_stdout_bytes(3)
            .max_stderr_bytes(2)
            .fail_on_output_truncation(true)
            .run(Command::shell("printf abcdef; printf wxyz >&2"))
            .expect_err("truncated successful output should be rejected");

        assert_eq!(error.kind(), CommandErrorKind::OutputTruncated);
        let output = error
            .output()
            .expect("truncation error should expose output");
        assert_eq!(output.stdout(), b"abc");
        assert_eq!(output.stderr(), b"wx");
        assert!(output.stdout_truncated());
        assert!(output.stderr_truncated());
    }

    #[test]
    fn test_command_runner_bounded_output_limits_streams_and_rejects_truncation()
     {
        let runner =
            CommandRunner::new(Duration::from_secs(10)).bounded_output(3);

        assert_eq!(runner.configured_max_stdout_bytes(), Some(3));
        assert_eq!(runner.configured_max_stderr_bytes(), Some(3));
        assert!(runner.is_output_truncation_failure_enabled());

        let error = runner
            .run(Command::shell("printf abcdef; printf wxyz >&2"))
            .expect_err("bounded output should reject truncation");
        assert_eq!(error.kind(), CommandErrorKind::OutputTruncated);
    }

    #[test]
    fn test_command_runner_unexpected_exit_precedes_output_truncation() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .max_output_bytes(3)
            .fail_on_output_truncation(true)
            .run(Command::shell("printf abcdef; exit 7"))
            .expect_err("unexpected exit should be rejected");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert!(
            error
                .output()
                .expect("unexpected exit should expose output")
                .stdout_truncated()
        );
    }

    #[test]
    fn test_command_runner_run_tees_output_to_files() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let stdout_path = temp_dir.path().join("stdout.txt");
        let stderr_path = temp_dir.path().join("stderr.txt");

        let output = CommandRunner::new(Duration::from_secs(10))
            .max_output_bytes(3)
            .fail_on_output_truncation(false)
            .run_with(
                Command::shell("printf abcdef; printf wxyz >&2"),
                CommandRunOptions::new()
                    .tee_stdout_to_file(stdout_path.clone())
                    .tee_stderr_to_file(stderr_path.clone()),
            )
            .expect("command should run successfully");

        assert_eq!(output.stdout(), b"abc");
        assert_eq!(output.stderr(), b"wxy");
        assert_eq!(
            fs::read(&stdout_path).expect("stdout tee file should be readable"),
            b"abcdef",
        );
        assert_eq!(
            fs::read(&stderr_path).expect("stderr tee file should be readable"),
            b"wxyz",
        );
    }

    #[test]
    fn test_command_runner_run_reports_output_file_open_failure() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let path = temp_dir.path().join("missing-dir").join("stdout.txt");
        let error = CommandRunner::new(Duration::from_secs(10))
            .run_with(
                Command::shell("printf ignored"),
                CommandRunOptions::new().tee_stdout_to_file(path.clone()),
            )
            .expect_err("missing output directory should be reported");

        match error.reason() {
            CommandErrorReason::OpenOutputFailed {
                stream,
                path: actual_path,
                ..
            } => {
                assert_eq!(*stream, OutputStream::Stdout);
                assert_eq!(actual_path, &path);
            }
            other => panic!("expected stdout open failure, got {other:?}"),
        }
    }

    #[test]
    fn test_command_runner_run_reports_stderr_file_open_failure() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let path = temp_dir.path().join("missing-dir").join("stderr.txt");
        let error = CommandRunner::new(Duration::from_secs(10))
            .run_with(
                Command::shell("printf ignored"),
                CommandRunOptions::new().tee_stderr_to_file(path.clone()),
            )
            .expect_err("missing output directory should be reported");

        match error.reason() {
            CommandErrorReason::OpenOutputFailed {
                stream,
                path: actual_path,
                ..
            } => {
                assert_eq!(*stream, OutputStream::Stderr);
                assert_eq!(actual_path, &path);
            }
            other => panic!("expected stderr open failure, got {other:?}"),
        }
    }

    #[test]
    fn test_command_runner_run_reports_spawn_failure() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::new("__qubit_command_missing_executable__"))
            .expect_err("missing executable should fail to spawn");

        assert_eq!(error.kind(), CommandErrorKind::SpawnFailed);
    }

    #[test]
    fn test_command_runner_error_uses_argv_style_command_text() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("two words"),
            )
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"["__qubit_command_missing_executable__", "two words"]"#,
        );
    }

    #[test]
    fn test_command_runner_error_redacts_sensitive_argv_values() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--password")
                    .arg("secret")
                    .arg("--access-token=abcdef")
                    .arg("OPENAI_API_KEY=uvwxyz")
                    .arg("--mode")
                    .arg("debug"),
            )
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"["__qubit_command_missing_executable__", "--password", "<redacted>", "--access-token=****", "OPENAI_API_KEY=****", "--mode", "debug"]"#,
        );
        assert!(!error.command().contains("secret"));
        assert!(!error.command().contains("abcdef"));
        assert!(!error.command().contains("uvwxyz"));
    }

    #[test]
    fn test_command_runner_error_redacts_sensitive_jvm_property() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("-Dpassword=jvm-secret"),
            )
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"["__qubit_command_missing_executable__", "-Dpassword=<redacted>"]"#,
        );
        assert!(!error.command().contains("jvm-secret"));
    }

    #[test]
    fn test_command_runner_error_masks_sensitive_option_after_double_dash() {
        let error =
            CommandRunner::new(Duration::from_secs(10))
                .run(
                    Command::new("__qubit_command_missing_executable__")
                        .args(&["--", "child", "--password", "raw-secret"]),
                )
                .expect_err("missing executable should fail to spawn");

        let display = error.to_string();
        let debug = format!("{error:?}");
        for rendering in [error.command(), display.as_str(), debug.as_str()] {
            assert!(!rendering.contains("raw-secret"));
            assert!(rendering.contains("<redacted>"));
        }
    }

    #[test]
    fn test_command_runner_error_redacts_shell_payload() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("printf ignored; printf hunter2 >&2; exit 9"))
            .expect_err("non-success shell command should fail");

        assert_eq!(error.command(), r#"["sh", "-c", "<redacted>"]"#);
        assert!(!error.command().contains("hunter2"));
    }

    #[test]
    fn test_command_runner_error_redacts_environment_display() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .env("OPENAI_API_KEY", "abcdef")
                    .env("MODE", "debug")
                    .env_remove("OLD_TOKEN"),
            )
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"Command { env: ["OPENAI_API_KEY=****", "MODE=debug"], unset: ["OLD_TOKEN"], argv: ["__qubit_command_missing_executable__"] }"#,
        );
        assert!(!error.command().contains("abcdef"));
    }

    #[test]
    fn test_command_runner_error_redacts_default_database_credentials() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--passphrase")
                    .arg("argv-secret")
                    .env("PGPASSWORD", "env-secret"),
            )
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"Command { env: ["PGPASSWORD=<redacted>"], unset: [], argv: ["__qubit_command_missing_executable__", "--passphrase", "<redacted>"] }"#,
        );
        assert!(!error.command().contains("argv-secret"));
        assert!(!error.command().contains("env-secret"));
    }

    #[test]
    fn test_command_runner_error_redacts_configured_sensitive_field() {
        let mut builder = RedactionPolicy::default().to_builder();
        builder
            .edit_fields()
            .raise("tenant_option", Sensitivity::Secret)
            .expect("the test policy field must be valid");
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--tenant-option")
                    .arg("argv-secret")
                    .env("TENANT_OPTION", "env-secret"),
            )
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"Command { env: ["TENANT_OPTION=<redacted>"], unset: [], argv: ["__qubit_command_missing_executable__", "--tenant-option", "<redacted>"] }"#,
        );
        assert!(!error.command().contains("argv-secret"));
        assert!(!error.command().contains("env-secret"));
    }

    #[test]
    fn test_command_runner_error_redacts_multiple_configured_sensitive_fields()
    {
        let mut builder = RedactionPolicy::default().to_builder();
        builder
            .edit_fields()
            .raise("tenant_option", Sensitivity::Secret)
            .expect("the test policy field must be valid")
            .raise("tenant_env", Sensitivity::Secret)
            .expect("the test policy field must be valid");
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--tenant-option")
                    .arg("argv-secret")
                    .env("TENANT_ENV", "env-secret"),
            )
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"Command { env: ["TENANT_ENV=<redacted>"], unset: [], argv: ["__qubit_command_missing_executable__", "--tenant-option", "<redacted>"] }"#,
        );
        assert!(!error.command().contains("argv-secret"));
        assert!(!error.command().contains("env-secret"));
    }

    #[test]
    fn test_command_runner_floor_overrides_exact_allow_for_default_sensitive_fields()
     {
        let mut builder = RedactionPolicy::default().to_builder();
        builder
            .edit_fields()
            .allow_exact("sig")
            .expect("the test policy field must be valid")
            .allow_exact("signature")
            .expect("the test policy field must be valid");
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--sig")
                    .arg("known-false-positive")
                    .env("SIGNATURE", "known-env-false-positive"),
            )
            .expect_err("missing executable should fail to spawn");

        assert!(!error.command().contains("known-false-positive"));
        assert!(!error.command().contains("known-env-false-positive"));
    }

    #[test]
    fn test_command_runner_floor_overrides_suffix_allow_for_default_sensitive_fields()
     {
        let mut builder = RedactionPolicy::default().to_builder();
        builder
            .edit_fields()
            .allow_suffix("access_token")
            .expect("the test policy field must be valid");
        let policy = builder
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--openai-access-token")
                    .arg("known-argv-false-positive")
                    .env("OPENAI_ACCESS_TOKEN", "known-env-false-positive"),
            )
            .expect_err("missing executable should fail to spawn");

        assert!(!error.command().contains("known-argv-false-positive"));
        assert!(!error.command().contains("known-env-false-positive"));
    }
}

#[cfg(windows)]
mod windows {
    use std::thread;

    use super::Command;
    use super::CommandCancellation;
    use super::CommandRunner;
    use super::Duration;
    use super::Instant;
    use super::LocalTempDir;
    use super::fs;

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

    #[test]
    fn test_command_runner_windows_captures_stdout() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("echo command-out"))
            .expect("Windows shell command should run successfully");

        assert_eq!(
            trim_windows_line_endings(
                output.stdout_text().expect("stdout should be UTF-8")
            ),
            "command-out",
        );
    }

    #[test]
    fn test_command_runner_windows_captures_stderr() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("echo command-error>&2"))
            .expect("Windows shell command should run successfully");

        assert_eq!(
            trim_windows_line_endings(
                output.stderr_text().expect("stderr should be UTF-8")
            ),
            "command-error",
        );
    }

    #[test]
    fn test_command_runner_windows_reports_timeout() {
        let error = CommandRunner::new(Duration::from_millis(50))
            .run(Command::shell("ping -n 3 127.0.0.1 >NUL"))
            .expect_err("long-running Windows command should time out");

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
    }

    #[test]
    fn test_command_runner_windows_cancels_running_command() {
        let cancellation = CommandCancellation::new();
        let cancellation_request = cancellation.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            cancellation_request.cancel();
        });
        let started = Instant::now();

        let error = CommandRunner::without_timeout()
            .run_with(
                Command::shell("ping -n 30 127.0.0.1 >NUL"),
                CommandRunOptions::new().cancellation(cancellation),
            )
            .expect_err("long-running Windows command should be cancelled");
        canceller
            .join()
            .expect("Windows cancellation thread should finish");

        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancellation should not wait for the command to exit normally",
        );
    }

    #[test]
    fn test_command_runner_windows_times_out_when_background_child_inherits_output()
     {
        let started = Instant::now();
        let error = CommandRunner::new(Duration::from_millis(250))
            .run(Command::shell("start \"\" /B ping -n 6 127.0.0.1"))
            .expect_err(
                "background child with inherited output should time out",
            );

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout should not wait for the background child to exit",
        );
    }

    #[test]
    fn test_command_runner_windows_tees_output_to_file() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-windows-")
            .expect("command test temp directory should be created");
        let stdout_path = temp_dir.path().join("stdout.txt");
        let output = CommandRunner::new(Duration::from_secs(10))
            .max_stdout_bytes(3)
            .fail_on_output_truncation(false)
            .run_with(
                Command::shell("echo abcdef"),
                CommandRunOptions::new()
                    .tee_stdout_to_file(stdout_path.clone()),
            )
            .expect("Windows shell command should run successfully");

        assert_eq!(output.stdout(), b"abc");
        assert!(output.stdout_truncated());
        assert_eq!(
            trim_windows_line_endings(
                std::str::from_utf8(
                    &fs::read(&stdout_path)
                        .expect("tee file should be readable")
                )
                .expect("tee file should contain UTF-8"),
            ),
            "abcdef",
        );
    }
}
