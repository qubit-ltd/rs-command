// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandRunner`](qubit_command::CommandRunner).

use std::{
    fs,
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use qubit_command::OutputStream;
use qubit_command::{Command, CommandError, CommandRunner, DEFAULT_COMMAND_TIMEOUT};
use qubit_redact::{DiagnosticBudget, RedactionPolicy, Sensitivity};

mod command_runner;
mod support;
use support::LocalTempDir;

#[cfg(not(windows))]
mod unix {
    use super::{
        Command, CommandError, CommandRunner, DEFAULT_COMMAND_TIMEOUT, DiagnosticBudget, Duration,
        Instant, LocalTempDir, OutputStream, RedactionPolicy, Sensitivity, fs,
        support::{captured_log_records_containing, initialize_captured_logger},
    };

    #[test]
    fn test_command_runner_default_configuration() {
        let runner = CommandRunner::new();

        assert_eq!(runner.configured_timeout(), Some(DEFAULT_COMMAND_TIMEOUT),);
        assert_eq!(runner.configured_success_exit_codes(), &[0]);
        assert!(runner.configured_working_directory().is_none());
        assert!(!runner.is_logging_disabled());
        assert!(!runner.is_output_truncation_failure_enabled());
        assert_eq!(runner.configured_max_stdout_bytes(), None);
        assert_eq!(runner.configured_max_stderr_bytes(), None);
        assert!(runner.configured_stdout_file().is_none());
        assert!(runner.configured_stderr_file().is_none());
    }

    #[test]
    fn test_runner_accepts_a_complete_diagnostic_redaction_policy() {
        let policy = RedactionPolicy::builder()
            .raise("tenant_option", Sensitivity::Secret)
            .allow_exact("username")
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let runner = CommandRunner::new().diagnostic_redaction_policy(policy.clone());

        assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy,);
    }

    #[test]
    fn test_command_runner_shares_configured_diagnostic_input_budget() {
        let budget =
            DiagnosticBudget::new(3, 128).expect("the small diagnostic budget should be valid");
        let policy = RedactionPolicy::builder()
            .diagnostic_budget(budget)
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new()
            .diagnostic_redaction_policy(policy)
            .run(Command::new("xxx").env("A", "B").env_remove("C"))
            .expect_err("the missing executable should fail to spawn");

        assert!(error.command().contains(r#"argv: ["xxx"]"#));
        assert!(error.command().contains("<truncated>"));
        assert!(!error.command().contains("A=B"));
        assert!(!error.command().contains(r#""C""#));
    }

    #[test]
    fn test_command_runner_debug_describes_configuration() {
        let debug = format!("{:?}", CommandRunner::new());

        assert!(debug.contains("CommandRunner"));
        assert!(debug.contains("success_exit_codes: [0]"));
        assert!(debug.contains("timer: \"<dyn Timer>\""));
    }

    #[test]
    fn test_command_runner_debug_redacts_path_configuration() {
        let debug = format!(
            "{:?}",
            CommandRunner::new()
                .working_directory("customer/working-directory")
                .tee_stdout_to_file("customer/stdout.log")
                .tee_stderr_to_file("customer/stderr.log"),
        );

        assert!(debug.contains("working_directory: Some(\"<redacted path>\")"));
        assert!(debug.contains("stdout_file: Some(\"<redacted path>\")"));
        assert!(debug.contains("stderr_file: Some(\"<redacted path>\")"));
        assert!(!debug.contains("customer/working-directory"));
        assert!(!debug.contains("customer/stdout.log"));
        assert!(!debug.contains("customer/stderr.log"));
    }

    #[test]
    fn test_command_runner_run_captures_stdout() {
        let output = CommandRunner::new()
            .run(Command::shell("printf command-out"))
            .expect("command should run successfully");

        assert_eq!(output.exit_code(), Some(0));
        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "command-out",
        );
        assert!(output.stderr().is_empty());
    }

    #[test]
    fn test_runner_without_timeout_uses_parent_process_group() {
        let output = CommandRunner::new()
            .without_timeout()
            .run(Command::shell("ps -o pgid= -p $$; ps -o pgid= -p $PPID"))
            .expect("command without timeout should run successfully");
        let process_groups: Vec<i32> = output
            .stdout_text()
            .expect("process groups should be valid UTF-8")
            .split_whitespace()
            .map(|value| value.parse().expect("process group should be numeric"))
            .collect();

        assert_eq!(process_groups.len(), 2);
        assert_eq!(
            process_groups[0], process_groups[1],
            "a command without timeout must remain in its parent's process group",
        );
    }

    #[test]
    fn test_command_runner_run_captures_stderr() {
        let output = CommandRunner::new()
            .run(Command::shell("printf command-error >&2"))
            .expect("command should run successfully");

        assert!(output.stdout().is_empty());
        assert_eq!(
            output.stderr_text().expect("stderr should be valid UTF-8"),
            "command-error",
        );
    }

    #[test]
    fn test_command_runner_run_applies_environment_override() {
        let output = CommandRunner::new()
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
        let output = CommandRunner::new()
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
        let output = CommandRunner::new()
            .run(
                Command::shell("printf \"$QUBIT_COMMAND_TEST\"")
                    .env_clear()
                    .env("QUBIT_COMMAND_TEST", "after-clear"),
            )
            .expect("command should run with cleared environment plus explicit set");

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "after-clear",
        );
    }

    #[test]
    fn test_command_runner_run_applies_working_directory_override() {
        let output = CommandRunner::new()
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
        let output = CommandRunner::new()
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
                    output.stdout_text().expect("stdout should be valid UTF-8"),
                    "fail-out",
                );
                assert_eq!(
                    output.stderr_text().expect("stderr should be valid UTF-8"),
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
            .run(Command::shell("exit 7"))
            .expect("configured success exit code should be accepted");

        assert_eq!(output.exit_code(), Some(7));
    }

    #[test]
    fn test_command_runner_run_accepts_configured_success_codes() {
        let output = CommandRunner::new()
            .success_exit_codes(&[3, 7])
            .run(Command::shell("exit 3"))
            .expect("configured success exit code list should be accepted");

        assert_eq!(output.exit_code(), Some(3));
    }

    #[test]
    fn test_command_runner_run_without_timeout() {
        let output = CommandRunner::new()
            .without_timeout()
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
        use qubit_clock::{ManualMonotonicClock, MonotonicClock};

        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::new()
            .without_timeout()
            .timer(clock.new_timer());
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
        let output = CommandRunner::new()
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
        let output = CommandRunner::new()
            .run(Command::shell("true").stdin_bytes(input))
            .expect("closed stdin should not hide a successful exit");

        assert_eq!(output.exit_code(), Some(0));
    }

    #[test]
    fn test_command_runner_run_preserves_exit_status_after_stdin_broken_pipe() {
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
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let path = temp_dir.path().join("stdin.txt");
        fs::write(&path, b"stdin-file").expect("stdin fixture should be written");

        let output = CommandRunner::new()
            .run(Command::shell("cat").stdin_file(path.clone()))
            .expect("command should receive stdin file");

        assert_eq!(
            output.stdout_text().expect("stdout should be valid UTF-8"),
            "stdin-file",
        );
    }

    #[test]
    fn test_command_runner_run_accepts_stdin_inherit() {
        let output = CommandRunner::new()
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
    fn test_command_runner_output_limit_updates_configuration() {
        let runner = CommandRunner::new().max_stdout_bytes(3).max_stderr_bytes(4);

        assert_eq!(runner.configured_max_stdout_bytes(), Some(3));
        assert_eq!(runner.configured_max_stderr_bytes(), Some(4));
    }

    #[test]
    fn test_command_runner_fail_on_output_truncation_updates_configuration() {
        let runner = CommandRunner::new().fail_on_output_truncation(true);

        assert!(runner.is_output_truncation_failure_enabled());
    }

    #[test]
    fn test_command_runner_output_file_updates_configuration() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let stdout_path = temp_dir.path().join("stdout-config.txt");
        let stderr_path = temp_dir.path().join("stderr-config.txt");
        let runner = CommandRunner::new()
            .tee_stdout_to_file(stdout_path.clone())
            .tee_stderr_to_file(stderr_path.clone());

        assert_eq!(runner.configured_stdout_file(), Some(stdout_path.as_path()));
        assert_eq!(runner.configured_stderr_file(), Some(stderr_path.as_path()));
    }

    #[test]
    fn test_command_runner_run_logs_success_lifecycle_at_debug() {
        const MARKER: &str = "qubit-command-log-success-marker";
        initialize_captured_logger();

        let output = CommandRunner::new()
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

        let output = CommandRunner::new()
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

        let error = CommandRunner::new()
            .run(Command::new("sh").arg("-c").arg("exit 8").arg(MARKER))
            .expect_err("unexpected exit should be reported");

        assert!(matches!(error, CommandError::UnexpectedExit { .. }));
        let records = captured_log_records_containing(MARKER);
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|(level, _)| *level == log::Level::Debug));
    }

    #[test]
    fn test_command_runner_run_suppresses_failure_logging() {
        const MARKER: &str = "qubit-command-log-quiet-failure-marker";
        initialize_captured_logger();

        let error = CommandRunner::new()
            .disable_logging(true)
            .run(Command::new("sh").arg("-c").arg("exit 8").arg(MARKER))
            .expect_err("unexpected exit should still be reported when logging is disabled");

        assert!(matches!(error, CommandError::UnexpectedExit { .. }));
        assert!(captured_log_records_containing(MARKER).is_empty());
    }

    #[test]
    fn test_command_runner_run_logs_redacted_command_text() {
        const MARKER: &str = "qubit-command-log-redacted-marker";
        const SECRET: &str = "command-log-secret";
        initialize_captured_logger();

        let output = CommandRunner::new()
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
    fn test_runner_timeout_uses_injected_manual_timer() {
        use qubit_clock::{ManualMonotonicClock, MonotonicClock};

        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::new()
            .timeout(Duration::from_secs(30))
            .timer(clock.new_timer());
        let worker = std::thread::spawn(move || runner.run(Command::shell("sleep 60")));

        assert!(clock.wait_for_waiters(1, Duration::from_secs(2)));
        clock
            .advance(Duration::from_secs(30))
            .expect("manual time should advance");
        let error = worker
            .join()
            .expect("runner thread should not panic")
            .expect_err("command should time out");
        assert!(matches!(error, CommandError::TimedOut { .. }));
    }

    #[test]
    fn test_runner_timeout_accepts_child_that_exits_before_deadline() {
        use qubit_clock::{ManualMonotonicClock, MonotonicClock};

        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let signal_path = temp_dir.path().join("deadline-signal");
        let completion_path = temp_dir.path().join("deadline-completion");
        let script = "while [ ! -e \"$1\" ]; do sleep 0.01; done; : > \"$2\"";
        let clock = ManualMonotonicClock::new_shared();
        let timeout = Duration::from_secs(30);
        let runner = CommandRunner::new()
            .timeout(timeout)
            .timer(clock.new_timer());
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
        fs::write(&signal_path, b"release").expect("signal file should release child command");
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
        let _ = result.expect("a child that exits before the deadline should complete normally");
    }

    #[test]
    fn test_command_runner_timer_updates_configuration() {
        use qubit_clock::{ManualMonotonicClock, MonotonicClock};

        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::new().timer(clock.new_timer());

        assert_eq!(
            runner.configured_timer().clock().now().domain(),
            clock.now().domain(),
        );
    }

    #[test]
    fn test_command_runner_run_kills_process_group_on_timeout() {
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
    fn test_command_runner_run_times_out_when_background_child_inherits_output() {
        let start = Instant::now();
        let error = CommandRunner::new()
            .timeout(Duration::from_millis(50))
            .run(Command::shell("sleep 2 &"))
            .expect_err("background child with inherited output pipes should time out");

        assert!(matches!(error, CommandError::TimedOut { .. }));
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "timeout should include output collection after the direct child exits",
        );
    }

    #[test]
    fn test_command_runner_run_limits_captured_output() {
        let output = CommandRunner::new()
            .max_stdout_bytes(3)
            .max_stderr_bytes(2)
            .run(Command::shell("printf abcdef; printf wxyz >&2"))
            .expect("command should run successfully");

        assert_eq!(output.stdout(), b"abc");
        assert_eq!(output.stderr(), b"wx");
        assert!(output.stdout_truncated());
        assert!(output.stderr_truncated());
    }

    #[test]
    fn test_command_runner_run_fails_when_output_is_truncated() {
        let error = CommandRunner::new()
            .max_stdout_bytes(3)
            .max_stderr_bytes(2)
            .fail_on_output_truncation(true)
            .run(Command::shell("printf abcdef; printf wxyz >&2"))
            .expect_err("truncated successful output should be rejected");

        assert!(matches!(error, CommandError::OutputTruncated { .. }));
        let output = error
            .output()
            .expect("truncation error should expose output");
        assert_eq!(output.stdout(), b"abc");
        assert_eq!(output.stderr(), b"wx");
        assert!(output.stdout_truncated());
        assert!(output.stderr_truncated());
    }

    #[test]
    fn test_command_runner_unexpected_exit_precedes_output_truncation() {
        let error = CommandRunner::new()
            .max_output_bytes(3)
            .fail_on_output_truncation(true)
            .run(Command::shell("printf abcdef; exit 7"))
            .expect_err("unexpected exit should be rejected");

        assert!(matches!(error, CommandError::UnexpectedExit { .. }));
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

        let output = CommandRunner::new()
            .max_output_bytes(3)
            .tee_stdout_to_file(stdout_path.clone())
            .tee_stderr_to_file(stderr_path.clone())
            .run(Command::shell("printf abcdef; printf wxyz >&2"))
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
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temp directory should be created");
        let path = temp_dir.path().join("missing-dir").join("stderr.txt");
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
        let error = CommandRunner::new()
            .run(Command::new("__qubit_command_missing_executable__"))
            .expect_err("missing executable should fail to spawn");

        assert!(matches!(error, CommandError::SpawnFailed { .. }));
    }

    #[test]
    fn test_command_runner_error_uses_argv_style_command_text() {
        let error = CommandRunner::new()
            .run(Command::new("__qubit_command_missing_executable__").arg("two words"))
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"["__qubit_command_missing_executable__", "two words"]"#,
        );
    }

    #[test]
    fn test_command_runner_error_redacts_sensitive_argv_values() {
        let error = CommandRunner::new()
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
    fn test_command_runner_error_masks_sensitive_option_after_double_dash() {
        let error = CommandRunner::new()
            .run(Command::new("__qubit_command_missing_executable__").args(&[
                "--",
                "child",
                "--password",
                "raw-secret",
            ]))
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
        let error = CommandRunner::new()
            .run(Command::shell("printf ignored; printf hunter2 >&2; exit 9"))
            .expect_err("non-success shell command should fail");

        assert_eq!(error.command(), r#"["sh", "-c", "<redacted>"]"#);
        assert!(!error.command().contains("hunter2"));
    }

    #[test]
    fn test_command_runner_error_redacts_environment_display() {
        let error = CommandRunner::new()
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
        let error = CommandRunner::new()
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
        let policy = RedactionPolicy::builder()
            .raise("tenant_option", Sensitivity::Secret)
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new()
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
    fn test_command_runner_error_redacts_multiple_configured_sensitive_fields() {
        let policy = RedactionPolicy::builder()
            .raise("tenant_option", Sensitivity::Secret)
            .raise("tenant_env", Sensitivity::Secret)
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new()
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
    fn test_command_runner_can_allow_exact_default_sensitive_fields() {
        let policy = RedactionPolicy::builder()
            .allow_exact("sig")
            .allow_exact("signature")
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new()
            .diagnostic_redaction_policy(policy)
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--sig")
                    .arg("known-false-positive")
                    .env("SIGNATURE", "known-env-false-positive"),
            )
            .expect_err("missing executable should fail to spawn");

        assert!(error.command().contains("known-false-positive"));
        assert!(error.command().contains("known-env-false-positive"));
    }

    #[test]
    fn test_command_runner_suffix_allow_wins_over_sensitive_suffix() {
        let policy = RedactionPolicy::builder()
            .allow_suffix("access_token")
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let error = CommandRunner::new()
            .diagnostic_redaction_policy(policy)
            .run(
                Command::new("__qubit_command_missing_executable__")
                    .arg("--openai-access-token")
                    .arg("known-argv-false-positive")
                    .env("OPENAI_ACCESS_TOKEN", "known-env-false-positive"),
            )
            .expect_err("missing executable should fail to spawn");

        assert!(error.command().contains("known-argv-false-positive"));
        assert!(error.command().contains("known-env-false-positive"));
    }
}

#[cfg(windows)]
mod windows {
    use super::{Command, CommandError, CommandRunner, Duration, Instant, LocalTempDir, fs};

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
        let output = CommandRunner::new()
            .run(Command::shell("echo command-out"))
            .expect("Windows shell command should run successfully");

        assert_eq!(
            trim_windows_line_endings(output.stdout_text().expect("stdout should be UTF-8")),
            "command-out",
        );
    }

    #[test]
    fn test_command_runner_windows_captures_stderr() {
        let output = CommandRunner::new()
            .run(Command::shell("echo command-error>&2"))
            .expect("Windows shell command should run successfully");

        assert_eq!(
            trim_windows_line_endings(output.stderr_text().expect("stderr should be UTF-8")),
            "command-error",
        );
    }

    #[test]
    fn test_command_runner_windows_reports_timeout() {
        let error = CommandRunner::new()
            .timeout(Duration::from_millis(50))
            .run(Command::shell("ping -n 3 127.0.0.1 >NUL"))
            .expect_err("long-running Windows command should time out");

        assert!(matches!(error, CommandError::TimedOut { .. }));
    }

    #[test]
    fn test_command_runner_windows_times_out_when_background_child_inherits_output() {
        let started = Instant::now();
        let error = CommandRunner::new()
            .timeout(Duration::from_millis(250))
            .run(Command::shell("start \"\" /B ping -n 6 127.0.0.1"))
            .expect_err("background child with inherited output should time out");

        assert!(matches!(error, CommandError::TimedOut { .. }));
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
        let output = CommandRunner::new()
            .max_stdout_bytes(3)
            .tee_stdout_to_file(stdout_path.clone())
            .run(Command::shell("echo abcdef"))
            .expect("Windows shell command should run successfully");

        assert_eq!(output.stdout(), b"abc");
        assert!(output.stdout_truncated());
        assert_eq!(
            trim_windows_line_endings(
                std::str::from_utf8(&fs::read(&stdout_path).expect("tee file should be readable"))
                    .expect("tee file should contain UTF-8"),
            ),
            "abcdef",
        );
    }
}
