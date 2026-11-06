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

#[cfg(not(windows))]
use qubit_clock::ManualMonotonicClock;
#[cfg(not(windows))]
use qubit_clock::MonotonicClock;
use qubit_command::Command;
use qubit_command::CommandCancellation;
use qubit_command::CommandErrorKind;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
#[cfg(not(windows))]
use qubit_command::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM;
use qubit_command::OutputStream;
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
use support::temp_dir::LocalTempDir;

#[test]
fn test_runner_pre_cancelled_command_does_not_prepare_output_file() {
    let temp_dir =
        LocalTempDir::with_prefix("qubit-command-test-").expect("command test temporary directory should be created");
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
    let temp_dir =
        LocalTempDir::with_prefix("qubit-command-test-").expect("command test temporary directory should be created");
    let stdout_path = temp_dir.path().join("stdout.log");
    fs::write(&stdout_path, b"must-survive-cancellation").expect("tee fixture should be written");
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
    let temp_dir =
        LocalTempDir::with_prefix("qubit-command-test-").expect("command test temporary directory should be created");

    let error = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("true").stdin_file(temp_dir.path()))
        .expect_err("a directory must not be used as command stdin");

    assert_eq!(error.kind(), CommandErrorKind::NonRegularInputFile);
}

#[test]
fn test_command_runner_rejects_directory_as_stdout_tee() {
    let temp_dir =
        LocalTempDir::with_prefix("qubit-command-test-").expect("command test temporary directory should be created");

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
    let temp_dir =
        LocalTempDir::with_prefix("qubit-command-test-").expect("command test temporary directory should be created");

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
    use super::Instant;
    use super::LocalTempDir;
    use super::ManualMonotonicClock;
    use super::MonotonicClock;
    use super::OsStr;
    use super::OutputStream;
    use super::RedactionPolicy;
    use super::Redactor;
    use super::Sensitivity;
    use super::fs;
    use super::support::captured_logger::captured_log_records_containing;
    use super::support::captured_logger::initialize_captured_logger;
    #[cfg(target_os = "linux")]
    use super::support::escaped_process_guard::EscapedProcessGuard;

    #[cfg(target_os = "linux")]
    fn linux_process_is_running(pid: u32) -> bool {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let Some((_, fields)) = stat.rsplit_once(')') else {
            return true;
        };
        !matches!(fields.split_whitespace().next(), Some("Z" | "X"))
    }

    #[cfg(target_os = "linux")]
    fn run_timeout_fixture_with_watchdog(pid_path: &std::path::Path, fixture: &str) {
        const CHILD_ENV: &str = "QUBIT_COMMAND_TIMEOUT_FIXTURE";
        const PID_ENV: &str = "QUBIT_COMMAND_TIMEOUT_FIXTURE_PID";
        const WATCHDOG: Duration = Duration::from_secs(30);

        if let Ok(child_fixture) = std::env::var(CHILD_ENV) {
            let child_pid_path = std::env::var_os(PID_ENV)
                .map(std::path::PathBuf::from)
                .expect("watchdog parent should provide the PID handshake path");
            run_timeout_fixture(&child_fixture, &child_pid_path);
            return;
        }

        let current_thread = std::thread::current();
        let test_name = current_thread.name().expect("test thread should be named");
        let mut escaped = EscapedProcessGuard::new(pid_path.to_path_buf());
        let watchdog = WATCHDOG;
        let mut child = std::process::Command::new(
            std::env::current_exe().expect("integration test executable should be available"),
        )
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, fixture)
        .env(PID_ENV, pid_path)
        .spawn()
        .expect("timeout fixture subprocess should start");
        let deadline = Instant::now() + watchdog;
        loop {
            match child
                .try_wait()
                .expect("timeout fixture subprocess should be observable")
            {
                Some(status) => {
                    let cleaned = escaped
                        .terminate_if_recorded()
                        .expect("escaped descendant should be cleaned up after fixture exit");
                    if fixture == "watchdog-self-test" {
                        assert!(!status.success(), "stalled fixture should be terminated by watchdog");
                        assert!(cleaned, "watchdog should find and clean the escaped descendant");
                    } else {
                        assert!(status.success(), "timeout fixture failed: {status}");
                    }
                    return;
                }
                None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let cleaned = escaped
                        .terminate_if_recorded()
                        .expect("watchdog should clean up escaped descendant");
                    if fixture == "watchdog-self-test" {
                        assert!(cleaned, "watchdog should find and clean the escaped descendant");
                        return;
                    }
                    panic!("timeout fixture exceeded {watchdog:?} watchdog");
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn run_timeout_fixture(fixture: &str, pid_path: &std::path::Path) {
        const READY_TIMEOUT: Duration = Duration::from_secs(5);
        const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
        const TIMEOUT: Duration = Duration::from_secs(30);

        if fixture == "watchdog-self-test" {
            let escaped = EscapedProcessGuard::new(pid_path.to_path_buf());
            let mut stalled = std::process::Command::new("setsid")
                .arg("sh")
                .arg("-c")
                .arg("echo \"$$\" > \"$1\"; exec sleep 60")
                .arg("sh")
                .arg(pid_path)
                .spawn()
                .expect("stalled watchdog fixture should start");
            escaped.wait_until_recorded(READY_TIMEOUT);
            let _ = stalled.wait();
        }

        let stderr = fixture == "stderr";
        let stdin = fixture == "stdin";
        let ready_path = pid_path.with_extension("ready");
        let parent_pid_path = pid_path.with_extension("parent.pid");
        let mut escaped = EscapedProcessGuard::new(pid_path.to_path_buf());
        let script = if fixture == "ready-timeout" {
            "echo \"$$\" > \"$3.tmp\"; mv \"$3.tmp\" \"$3\"; setsid sh -c 'echo \"$$\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; exec sleep 60' sh \"$1\" & printf started"
        } else if stdin {
            "echo \"$$\" > \"$3.tmp\"; mv \"$3.tmp\" \"$3\"; setsid sh -c 'echo \"$$\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; : > \"$2\"; exec sleep 60' sh \"$1\" \"$2\" >/dev/null 2>&1 & wait"
        } else if stderr {
            "echo \"$$\" > \"$3.tmp\"; mv \"$3.tmp\" \"$3\"; setsid sh -c 'echo \"$$\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; : > \"$2\"; exec sleep 60' sh \"$1\" \"$2\" >/dev/null & printf started >&2"
        } else {
            "echo \"$$\" > \"$3.tmp\"; mv \"$3.tmp\" \"$3\"; setsid sh -c 'echo \"$$\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; : > \"$2\"; exec sleep 60' sh \"$1\" \"$2\" & printf started"
        };
        let command = if stdin {
            Command::shell(script)
                .arg("sh")
                .arg_os(pid_path)
                .arg_os(&ready_path)
                .arg_os(&parent_pid_path)
                .stdin_bytes(vec![b'x'; 4 * 1024 * 1024])
        } else {
            Command::new("sh")
                .arg("-c")
                .arg(script)
                .arg("sh")
                .arg_os(pid_path)
                .arg_os(&ready_path)
                .arg_os(&parent_pid_path)
        };
        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::new(TIMEOUT).timer(clock.new_timer());
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker_fixture = fixture.to_owned();
        let worker = std::thread::spawn(move || {
            let result = runner.run(command);
            if worker_fixture == "worker-panic" {
                panic!("injected timeout result-worker panic: {result:?}");
            }
            sender.send(result)
        });

        let readiness = escaped.try_wait_until_recorded(READY_TIMEOUT);
        if readiness.is_ok() {
            let deadline = Instant::now() + READY_TIMEOUT;
            while (fixture == "ready-timeout" || !ready_path.exists()) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let ready = readiness.and_then(|()| {
            if fixture == "ready-timeout" {
                return Err("injected ready marker omission".to_owned());
            }
            if ready_path.exists() {
                Ok(())
            } else {
                Err("ready marker missing".to_owned())
            }
        });
        let direct_exit = if !stdin {
            let deadline = Instant::now() + READY_TIMEOUT;
            loop {
                let exited = fs::read_to_string(&parent_pid_path)
                    .ok()
                    .and_then(|pid| pid.trim().parse::<u32>().ok())
                    .is_some_and(|pid| !linux_process_is_running(pid));
                if exited {
                    break Ok(());
                }
                if Instant::now() >= deadline {
                    break Err("direct command did not exit".to_owned());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        } else {
            Ok(())
        };

        let waiting = direct_exit.is_ok() && clock.wait_for_waiters(1, READY_TIMEOUT);
        let advanced = if waiting && fixture != "no-advance" && fixture != "ready-timeout" {
            Some(clock.advance(TIMEOUT))
        } else {
            None
        };
        let observed = receiver.recv_timeout(RESPONSE_TIMEOUT);
        let observed_before_release = observed.is_ok();
        let cleanup = escaped.terminate_if_recorded();
        if fixture == "ready-timeout" || fixture == "no-advance" {
            clock
                .advance(Duration::from_millis(100))
                .expect("post-release poll tick should advance manual time");
        }
        let after_release = if observed_before_release {
            None
        } else {
            receiver.recv_timeout(RESPONSE_TIMEOUT).ok()
        };
        assert!(
            cleanup.expect("escaped descendant cleanup should succeed"),
            "escaped descendant PID should be cleaned"
        );
        direct_exit.expect("direct command should exit before timeout");
        if fixture == "ready-timeout" {
            assert!(ready.is_err(), "injected readiness failure should be reported");
            assert!(!observed_before_release, "readiness failure must not advance timeout");
            let natural_result = after_release.expect("runner should finish after readiness-failure cleanup");
            let output = natural_result.expect("releasing the fixture should complete naturally, not by timeout");
            assert!(
                output.stdout_complete(),
                "readiness-failure output should complete after release"
            );
            worker
                .join()
                .expect("runner worker should join after readiness-failure cleanup")
                .expect("result send should succeed");
            return;
        }
        if fixture == "worker-panic" {
            assert!(ready.is_ok(), "fixture readiness failed: {ready:?}");
            assert!(observed.is_err(), "panicking worker should disconnect result channel");
            assert!(worker.join().is_err(), "injected result worker panic should be joined");
            return;
        }
        if fixture == "no-advance" {
            assert!(ready.is_ok(), "fixture readiness failed: {ready:?}");
            assert!(waiting, "runner did not register its manual timeout waiter");
            assert!(
                !observed_before_release,
                "runner must not time out before manual time advances"
            );
            let natural_result = after_release.expect("natural completion after release should be observed");
            let output = natural_result.expect("release should complete naturally, not as a timeout");
            assert!(
                output.stdout_complete(),
                "no-advance output should complete after release"
            );
            worker
                .join()
                .expect("runner worker should join")
                .expect("result send should succeed");
            return;
        }
        assert!(ready.is_ok(), "fixture readiness failed: {ready:?}");
        assert!(waiting, "runner did not register its manual timeout waiter");
        advanced
            .expect("manual clock should advance")
            .expect("manual time advance should succeed");
        assert!(
            observed_before_release,
            "timeout result must arrive before releasing descendant"
        );
        let result = observed.expect("timeout worker result channel disconnected");
        worker
            .join()
            .expect("timeout result worker should join")
            .expect("timeout result send should succeed");
        let error = result.expect_err("escaped descendant should keep one stream incomplete");
        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        match error.reason() {
            CommandErrorReason::TimedOut { timeout } => assert_eq!(*timeout, TIMEOUT),
            other => panic!("expected timeout reason, got {other:?}"),
        }
        let output = error.output().expect("timeout should retain output");
        if stderr {
            assert!(!output.stderr_complete(), "stderr should remain incomplete");
            assert!(
                output.stderr().starts_with(b"started"),
                "stderr prefix should survive timeout"
            );
        } else if !stdin {
            assert!(!output.stdout_complete(), "stdout should remain incomplete");
            assert!(
                output.stdout().starts_with(b"started"),
                "stdout prefix should survive timeout"
            );
        }
        assert!(after_release.is_none(), "no second result should arrive after release");
    }

    #[cfg(target_os = "linux")]
    fn run_cancellation_fixture_with_watchdog(pid_path: &std::path::Path, fixture: &str) {
        const CHILD_ENV: &str = "QUBIT_COMMAND_CANCEL_FIXTURE";
        const PID_ENV: &str = "QUBIT_COMMAND_CANCEL_FIXTURE_PID";
        const WATCHDOG: Duration = Duration::from_secs(30);

        if std::env::var(CHILD_ENV).is_ok() {
            let child_pid_path = std::env::var_os(PID_ENV)
                .map(std::path::PathBuf::from)
                .expect("watchdog parent should provide the PID handshake path");
            run_cancellation_fixture(fixture, &child_pid_path);
            return;
        }

        let current_thread = std::thread::current();
        let test_name = current_thread.name().expect("test thread should be named");
        let mut escaped = EscapedProcessGuard::new(pid_path.to_path_buf());
        let mut child = std::process::Command::new(
            std::env::current_exe().expect("integration test executable should be available"),
        )
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, fixture)
        .env(PID_ENV, pid_path)
        .spawn()
        .expect("cancellation fixture subprocess should start");
        let deadline = Instant::now() + WATCHDOG;
        loop {
            match child.try_wait().expect("fixture subprocess should be observable") {
                Some(status) => {
                    if fixture == "late-pid" && !status.success() {
                        let _ = escaped.try_wait_until_recorded(Duration::from_secs(2));
                    }
                    escaped
                        .terminate_if_recorded()
                        .expect("escaped descendant should be cleaned up after fixture exit");
                    assert!(status.success(), "cancellation fixture failed: {status}");
                    return;
                }
                None if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    escaped
                        .terminate_if_recorded()
                        .expect("watchdog should clean up escaped descendant");
                    panic!("cancellation fixture exceeded {WATCHDOG:?} watchdog");
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn run_cancellation_fixture(fixture: &str, pid_path: &std::path::Path) {
        const READY_TIMEOUT: Duration = Duration::from_secs(5);
        const CANCEL_TIMEOUT: Duration = Duration::from_secs(2);
        const PRE_CANCEL_OBSERVATION: Duration = Duration::from_millis(100);

        let fixture_kind = match fixture {
            "delayed-readiness" | "late-pid" | "missing-cancel" | "worker-panic" => "stdout",
            other => other,
        };
        let parent_pid_path = pid_path.with_extension("parent.pid");
        let ready_path = pid_path.with_extension("ready");
        let release_path = pid_path.with_extension("release");
        let abort_path = pid_path.with_extension("abort");
        let abort_ack_path = pid_path.with_extension("abort-ack");
        let mut escaped = EscapedProcessGuard::new(pid_path.to_path_buf());
        let script = match fixture {
            "stdout" | "missing-cancel" | "worker-panic" => {
                "echo \"$$\" > \"$3.tmp\"; mv \"$3.tmp\" \"$3\"; setsid sh -c 'echo \"$$\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; : > \"$2\"; exec sleep 60' sh \"$1\" \"$2\" & printf started"
            }
            "delayed-readiness" => {
                "echo \"$$\" > \"$3.tmp\"; mv \"$3.tmp\" \"$3\"; setsid sh -c 'echo \"$$\" > \"$1.tmp\" && mv \"$1.tmp\" \"$1\" || { : > \"$5\"; exit 0; }; while [ ! -f \"$3\" ] && [ ! -f \"$4\" ]; do sleep 0.01; done; if [ -f \"$4\" ]; then : > \"$5\"; exec sleep 60; fi; : > \"$2\"; exec sleep 60' sh \"$1\" \"$2\" \"$4\" \"$5\" \"$6\" & printf started"
            }
            "late-pid" => {
                "echo \"$$\" > \"$3.tmp\"; mv \"$3.tmp\" \"$3\"; setsid sh -c 'sleep 0.5; echo \"$$\" > \"$1.tmp\" && mv \"$1.tmp\" \"$1\" || { : > \"$5\"; exit 0; }; while [ ! -f \"$3\" ] && [ ! -f \"$4\" ]; do sleep 0.01; done; if [ -f \"$4\" ]; then : > \"$5\"; exec sleep 60; fi; : > \"$2\"; exec sleep 60' sh \"$1\" \"$2\" \"$4\" \"$5\" \"$6\" & printf started"
            }
            "stdin" => {
                "setsid sh -c 'echo \"$$\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; : > \"$2\"; exec sleep 60' sh \"$1\" \"$2\" >/dev/null 2>&1 & wait"
            }
            _ => panic!("unknown cancellation fixture: {fixture}"),
        };
        let cancellation = CommandCancellation::new();
        let run_cancellation = cancellation.clone();
        let run_pid_path = pid_path.to_path_buf();
        let run_ready_path = ready_path.clone();
        let run_release_path = release_path.clone();
        let run_abort_path = abort_path.clone();
        let run_abort_ack_path = abort_ack_path.clone();
        let run_parent_pid_path = parent_pid_path.clone();
        let fixture_owned = fixture.to_owned();
        let command_fixture = fixture_kind.to_owned();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let command = if matches!(fixture_owned.as_str(), "delayed-readiness" | "late-pid") {
                Command::new("sh")
                    .arg("-c")
                    .arg(script)
                    .arg("sh")
                    .arg_os(&run_pid_path)
                    .arg_os(&run_ready_path)
                    .arg_os(&run_parent_pid_path)
                    .arg_os(&run_release_path)
                    .arg_os(&run_abort_path)
                    .arg_os(&run_abort_ack_path)
            } else {
                match command_fixture.as_str() {
                    "stdout" => Command::new("sh")
                        .arg("-c")
                        .arg(script)
                        .arg("sh")
                        .arg_os(&run_pid_path)
                        .arg_os(&run_ready_path)
                        .arg_os(&run_parent_pid_path),
                    "stdin" => Command::shell(script)
                        .arg("sh")
                        .arg_os(&run_pid_path)
                        .arg_os(&run_ready_path)
                        .stdin_bytes(vec![b'x'; 4 * 1024 * 1024]),
                    _ => unreachable!("fixture name was checked before worker start"),
                }
            };
            let result = CommandRunner::without_timeout()
                .run_with(command, CommandRunOptions::new().cancellation(run_cancellation));
            if fixture_owned == "worker-panic" {
                panic!("injected result-worker panic after runner cleanup: {result:?}");
            }
            sender
                .send(result)
                .expect("fixture result receiver should remain connected");
        });

        let ready_timeout = if matches!(fixture, "delayed-readiness" | "late-pid") {
            Duration::from_millis(100)
        } else {
            READY_TIMEOUT
        };
        let pid_handshake = if matches!(fixture, "delayed-readiness" | "late-pid") {
            escaped.try_wait_until_recorded(if fixture == "late-pid" {
                ready_timeout
            } else {
                READY_TIMEOUT
            })
        } else {
            Ok(())
        };
        let direct_exit = if matches!(fixture, "delayed-readiness" | "late-pid") {
            let deadline = Instant::now() + READY_TIMEOUT;
            loop {
                let exited = fs::read_to_string(&parent_pid_path)
                    .ok()
                    .and_then(|pid| pid.trim().parse::<u32>().ok())
                    .is_some_and(|pid| !linux_process_is_running(pid));
                if exited {
                    break Ok(());
                }
                if Instant::now() >= deadline {
                    break Err("direct command did not exit".to_owned());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        } else {
            Ok(())
        };
        let readiness = if fixture == "delayed-readiness" {
            if let Err(error) = &pid_handshake {
                Err(format!("escaped PID handshake failed: {error}"))
            } else if let Err(error) = &direct_exit {
                Err(error.clone())
            } else {
                let deadline = Instant::now() + ready_timeout;
                while Instant::now() < deadline && !ready_path.exists() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if ready_path.exists() {
                    Ok(())
                } else {
                    Err(format!("fixture readiness marker missing after {ready_timeout:?}"))
                }
            }
        } else if fixture == "late-pid" {
            Err(format!("escaped PID handshake failed: {pid_handshake:?}"))
        } else {
            escaped.try_wait_until_recorded(ready_timeout)
        };
        let post_release_ready = if fixture == "delayed-readiness" && pid_handshake.is_ok() && direct_exit.is_ok() {
            let release = fs::write(&release_path, b"release");
            let released_ready = if release.is_ok() {
                let deadline = Instant::now() + READY_TIMEOUT;
                while Instant::now() < deadline && !ready_path.exists() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                ready_path.exists()
            } else {
                false
            };
            Some((release.is_ok(), released_ready))
        } else {
            None
        };
        let abort_cleanup = if matches!(fixture, "delayed-readiness" | "late-pid")
            && (pid_handshake.is_err() || direct_exit.is_err())
        {
            let abort_written = fs::write(&abort_path, b"abort").is_ok();
            let deadline = Instant::now() + READY_TIMEOUT;
            let mut pid_recorded = false;
            while Instant::now() < deadline && !pid_recorded && !abort_ack_path.exists() {
                pid_recorded = fs::read_to_string(pid_path)
                    .ok()
                    .and_then(|contents| contents.trim().parse::<u32>().ok())
                    .is_some_and(|pid| pid != 0);
                if pid_recorded || abort_ack_path.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            while pid_recorded && Instant::now() < deadline && !abort_ack_path.exists() {
                std::thread::sleep(Duration::from_millis(10));
            }
            Some((abort_written, pid_recorded, abort_ack_path.exists()))
        } else {
            None
        };
        if readiness.is_ok() && fixture_kind == "stdout" {
            let parent_pid =
                fs::read_to_string(&parent_pid_path).expect("direct child should record its PID before exiting");
            let parent_pid = parent_pid.trim();
            let deadline = Instant::now() + READY_TIMEOUT;
            let mut parent_exited = false;
            while Instant::now() < deadline {
                let running = std::process::Command::new("kill")
                    .args(["-0", parent_pid])
                    .stderr(std::process::Stdio::null())
                    .status()
                    .expect("direct child exit should be observable")
                    .success();
                if !running {
                    parent_exited = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(parent_exited, "direct command process should exit before cancellation");
        }

        let before_cancel = receiver.recv_timeout(PRE_CANCEL_OBSERVATION);
        if fixture != "missing-cancel" {
            cancellation.cancel();
        }
        let cancel_started = Instant::now();
        let first_result = receiver.recv_timeout(CANCEL_TIMEOUT);
        let cancel_elapsed = cancel_started.elapsed();
        let observed_before_release = first_result.is_ok();

        if fixture == "worker-panic" {
            let cleanup_result = escaped.terminate_if_recorded();
            let worker_result = worker.join();
            cleanup_result.expect("escaped descendant cleanup should succeed after worker panic");
            assert!(readiness.is_ok(), "fixture readiness failed: {readiness:?}");
            assert!(matches!(before_cancel, Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
            assert!(matches!(
                first_result,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
            ));
            assert!(worker_result.is_err(), "injected result-worker panic should be joined");
            return;
        }

        if fixture == "missing-cancel" {
            let cleanup_result = escaped.terminate_if_recorded();
            let natural_result = match first_result {
                Ok(result) => Some(result),
                Err(_) => receiver.recv_timeout(CANCEL_TIMEOUT).ok(),
            };
            let worker_result = natural_result.map(|result| (result, worker.join()));
            cleanup_result.expect("fault-injected descendant cleanup should succeed");
            assert!(matches!(before_cancel, Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
            assert!(
                !observed_before_release,
                "missing cancellation must not appear responsive"
            );
            let (result, joined) = worker_result.expect("runner should finish after the pipe is released");
            let output = result.expect("natural EOF after release should complete the command");
            assert!(output.stdout().starts_with(b"started"));
            joined.expect("runner worker should return after natural EOF");
            return;
        }

        let readiness_pid = if readiness.is_err() && !matches!(fixture, "delayed-readiness" | "late-pid") {
            Some(escaped.try_wait_until_recorded(READY_TIMEOUT))
        } else {
            None
        };
        let cleanup_result = escaped.terminate_if_recorded();
        let result = match first_result {
            Ok(result) => Some(result),
            Err(_) => receiver.recv_timeout(CANCEL_TIMEOUT).ok(),
        };
        let worker_result = worker.join();

        let descendant_reclaimed = cleanup_result.expect("escaped descendant cleanup should succeed");
        if matches!(fixture, "delayed-readiness" | "late-pid") && (pid_handshake.is_err() || direct_exit.is_err()) {
            let (abort_written, pid_recorded, abort_acknowledged) =
                abort_cleanup.expect("preflight failure should run abort cleanup");
            assert!(
                abort_written,
                "abort marker should be written after a preflight failure"
            );
            assert!(
                pid_recorded || abort_acknowledged,
                "abort should be acknowledged or escaped PID should be recorded"
            );
            assert!(
                abort_acknowledged,
                "escaped child should acknowledge the abort path before cleanup"
            );
            assert!(
                descendant_reclaimed || (!pid_recorded && abort_acknowledged),
                "escaped descendant should be killed or confirm its natural abort exit"
            );
        }
        if fixture == "delayed-readiness" {
            assert!(pid_handshake.is_ok(), "escaped PID handshake failed: {pid_handshake:?}");
            assert!(direct_exit.is_ok(), "direct command did not exit: {direct_exit:?}");
            assert!(readiness.is_err(), "delayed fixture should miss the readiness deadline");
            assert_eq!(
                post_release_ready,
                Some((true, true)),
                "release should make the fixture ready"
            );
            assert!(
                descendant_reclaimed,
                "escaped descendant should be reclaimed after cancellation"
            );
        } else if fixture == "late-pid" {
            assert!(
                pid_handshake.is_err(),
                "late PID fixture should miss its preflight deadline"
            );
            assert!(
                direct_exit.is_ok(),
                "direct command should exit during late PID publication"
            );
            assert!(readiness.is_err(), "late PID should remain unavailable before abort");
            assert_eq!(post_release_ready, None, "preflight failure must not release readiness");
            assert!(
                !release_path.exists(),
                "late PID preflight failure must not create release marker"
            );
            assert!(
                descendant_reclaimed,
                "late escaped descendant should be killed after cancellation"
            );
        } else {
            assert!(readiness.is_ok(), "fixture readiness failed: {readiness:?}");
        }
        if let Some(readiness_pid) = readiness_pid {
            assert!(
                readiness_pid.is_ok(),
                "escaped PID should be available for failed-readiness cleanup"
            );
        }
        assert!(
            matches!(before_cancel, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "runner must remain active before cancellation"
        );
        assert!(
            observed_before_release,
            "runner should return within {CANCEL_TIMEOUT:?} before releasing the descendant"
        );
        assert!(
            cancel_elapsed < CANCEL_TIMEOUT,
            "cancellation exceeded its real-time bound"
        );
        let result = result.expect("runner result channel should remain connected");
        let error = result.expect_err("inherited pipe should make cancellation observable");
        worker_result.expect("runner worker should not panic");
        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        if fixture_kind == "stdout" {
            assert!(
                error
                    .output()
                    .expect("cancelled output should be retained")
                    .stdout()
                    .starts_with(b"started"),
                "captured stdout prefix should survive cancellation"
            );
        }
    }

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
        let runner = CommandRunner::new(Duration::from_secs(10)).unbounded_output();

        assert_eq!(runner.configured_max_stdout_bytes(), None);
        assert_eq!(runner.configured_max_stderr_bytes(), None);
        assert!(!runner.is_output_truncation_failure_enabled());

        let output = runner
            .run(Command::shell("head -c 1048577 /dev/zero"))
            .expect("explicitly unbounded output should retain the full stream");
        assert_eq!(output.stdout().len(), DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM + 1,);
        assert!(!output.stdout_truncated());
    }

    #[test]
    fn test_runner_accepts_a_complete_diagnostic_redaction_policy() {
        let policy = RedactionPolicy::default()
            .to_builder()
            .fields(|fields| {
                fields
                    .raise("tenant_option", Sensitivity::Secret)
                    .allow_exact("username");
            })
            .expect("the test policy field must be valid")
            .build()
            .expect("the diagnostic redaction policy should be valid");
        let runner = CommandRunner::new(Duration::from_secs(10)).diagnostic_redaction_policy(policy.clone());

        assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy,);
    }

    #[test]
    fn test_command_runner_keeps_complete_finite_diagnostic_inputs() {
        let policy = RedactionPolicy::default();
        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(Command::new("xxx").env("A", "B").env_remove("C"))
            .expect_err("the missing executable should fail to spawn");

        assert!(error.command().contains(r#"argv: ["xxx"]"#));
        assert!(error.command().contains("A=B"));
        assert!(error.command().contains(r#""C""#));
    }

    #[test]
    fn test_command_runner_keeps_long_diagnostic_values() {
        let policy = RedactionPolicy::default();
        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(
                Command::new("xxx")
                    .env("VISIBLE", "value")
                    .arg("argument-that-forces-the-full-diagnostic-to-truncate"),
            )
            .expect_err("the missing executable should fail to spawn");

        assert!(error.command().contains("argument-that-forces"));
    }

    #[test]
    fn test_command_runner_stages_each_named_adapter_result() {
        let policy = RedactionPolicy::default();
        let missing_program = "x".repeat(46);

        let redactor = Redactor::new(policy.clone());
        let mut batch = redactor.diagnostic_batch();
        let argv = batch.redact_heuristic_argv([ArgvItem::plain(OsStr::new(&missing_program))]);
        let env = batch.redact_env_pairs([(OsStr::new("MODE"), OsStr::new("debug"))]);
        let output = batch.finish_with_marker("<redaction incomplete>");
        assert_ne!(output.text(argv).as_str(), "<redaction incomplete>");
        assert_ne!(output.text(env).as_str(), "<redaction incomplete>");

        let error = CommandRunner::new(Duration::from_secs(10))
            .diagnostic_redaction_policy(policy)
            .run(Command::new(&missing_program).env("MODE", "debug"))
            .expect_err("the missing executable should fail to spawn");

        assert!(error.command().contains("env:"));
    }

    #[test]
    fn test_command_runner_debug_describes_configuration() {
        let debug = format!("{:?}", CommandRunner::new(Duration::from_secs(10)));

        assert!(debug.contains("CommandRunner"));
        assert!(debug.contains("success_exit_codes: [0]"));
        assert!(debug.contains("timer: \"<dyn Timer>\""));
    }

    #[test]
    fn test_command_runner_debug_redacts_path_configuration() {
        let debug = format!(
            "{:?}",
            CommandRunner::new(Duration::from_secs(10)).working_directory("customer/working-directory"),
        );
        let options_debug = format!(
            "{:?}",
            CommandRunOptions::new()
                .tee_stdout_to_file("customer/stdout.log")
                .tee_stderr_to_file("customer/stderr.log"),
        );

        assert!(debug.contains("working_directory: Some(\"<redacted path>\")"));
        assert!(options_debug.contains("stdout_file: Some(\"<redacted path>\")"));
        assert!(options_debug.contains("stderr_file: Some(\"<redacted path>\")"));
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
            .map(|value| value.parse().expect("process group should be numeric"))
            .collect();

        assert_eq!(process_groups.len(), 2);
        assert_eq!(
            process_groups[0], process_groups[1],
            "a command without timeout must remain in its parent's process group",
        );
    }

    #[test]
    fn test_runner_cancellation_without_timeout_terminates_running_command() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-managed-test-").expect("test temp directory should be created");
        let pid_path = temp_dir.path().join("command.pid");
        let test_name = std::thread::current()
            .name()
            .expect("test thread should be named")
            .to_owned();
        super::support::managed_test_process::run(&test_name, &pid_path, || {
            let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
                .expect("command test temp directory should be created");
            let started_path = temp_dir.path().join("started");
            let captured_path = temp_dir.path().join("captured");
            let tee_path = captured_path.clone();
            let pid_path = super::support::managed_test_process::command_pid_path();
            let abort_path = super::support::managed_test_process::command_abort_path();
            let abort_ack_path = super::support::managed_test_process::command_abort_ack_path();
            let script = format!(
                "{}printf started; : > '{}'; while :; do sleep 1; done",
                super::support::managed_test_process::shell_start_guard(),
                started_path.display(),
            );
            let cancellation = CommandCancellation::new();
            let runner = CommandRunner::without_timeout();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let run_cancellation = cancellation.clone();
            let worker = std::thread::spawn(move || {
                if !super::support::managed_test_process::permit_command_start() {
                    return;
                }
                let result = runner.run_with(
                    Command::new("sh")
                        .arg("-c")
                        .arg(&script)
                        .arg("sh")
                        .arg_os(&pid_path)
                        .arg_os(&abort_path)
                        .arg_os(&abort_ack_path),
                    CommandRunOptions::new()
                        .cancellation(run_cancellation)
                        .tee_stdout_to_file(tee_path),
                );
                sender.send(result).expect("test receiver should remain connected");
            });

            let deadline = Instant::now() + Duration::from_secs(5);
            while fs::read(&captured_path).ok().as_deref() != Some(b"started".as_slice()) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let ready =
                started_path.exists() && fs::read(&captured_path).ok().as_deref() == Some(b"started".as_slice());
            cancellation.cancel();
            let result = receiver.recv_timeout(Duration::from_secs(2));
            let join = worker.join();
            assert!(ready, "command output should be observed in tee before cancellation");
            let error = result
                .expect("cancelled runner should return promptly")
                .expect_err("cancelled command should return an error");
            join.expect("runner thread should not panic");

            assert_eq!(error.kind(), CommandErrorKind::Cancelled);
            assert_eq!(error.output().expect("cancelled output").stdout(), b"started");
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_wakes_silent_inherited_stdout_reader() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        run_cancellation_fixture_with_watchdog(&pid_path, "stdout");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_wakes_blocked_stdin_writer() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-stdin-child.pid");
        run_cancellation_fixture_with_watchdog(&pid_path, "stdin");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_reclaims_delayed_readiness_fixture() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        run_cancellation_fixture_with_watchdog(&pid_path, "delayed-readiness");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_aborts_late_pid_fixture_before_release() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        run_cancellation_fixture_with_watchdog(&pid_path, "late-pid");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_rejects_natural_eof_after_missing_cancel() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        run_cancellation_fixture_with_watchdog(&pid_path, "missing-cancel");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_runner_cancellation_observes_worker_panic_and_disconnect() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        run_cancellation_fixture_with_watchdog(&pid_path, "worker-panic");
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
            .run(Command::shell("printf \"$QUBIT_COMMAND_TEST\"").env("QUBIT_COMMAND_TEST", "from-env"))
            .expect("command should receive environment override");

        assert_eq!(output.stdout_text().expect("stdout should be valid UTF-8"), "from-env",);
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

        assert_eq!(output.stdout_text().expect("stdout should be valid UTF-8"), "missing",);
    }

    #[test]
    fn test_command_runner_run_applies_environment_clear_then_set() {
        let output = CommandRunner::new(Duration::from_secs(10))
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
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("pwd").working_directory("/"))
            .expect("command should run in requested working directory");

        assert_eq!(output.stdout_text().expect("stdout should be valid UTF-8").trim(), "/",);
    }

    #[test]
    fn test_command_runner_run_applies_default_working_directory() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .working_directory("/")
            .run(Command::shell("pwd"))
            .expect("command should run in runner working directory");

        assert_eq!(output.stdout_text().expect("stdout should be valid UTF-8").trim(), "/",);
    }

    #[test]
    fn test_command_runner_run_reports_unexpected_exit() {
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("printf fail-out; printf fail-err >&2; exit 7"))
            .expect_err("non-success exit code should be rejected");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert_eq!(error.exit_code(), Some(7));
        assert!(matches!(
            error.reason(),
            CommandErrorReason::UnexpectedExit { expected, .. } if expected == &[0]
        ));
        let output = error.output().expect("unexpected output");
        assert_eq!(output.stdout_text().expect("stdout should be valid UTF-8"), "fail-out");
        assert_eq!(output.stderr_text().expect("stderr should be valid UTF-8"), "fail-err");
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
        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::without_timeout().timer(clock.new_timer());
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = runner.run(Command::shell("sleep 0.05"));
            sender.send(result).expect("test receiver should remain connected");
        });

        let first_result = receiver.recv_timeout(Duration::from_secs(2));
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
            .expect_err("non-success exit should remain visible after stdin closes");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert_eq!(error.exit_code(), Some(7));
        assert!(matches!(
            error.reason(),
            CommandErrorReason::UnexpectedExit { expected, .. } if expected == &[0]
        ));
    }

    #[test]
    fn test_command_runner_run_reads_stdin_file() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-test-").expect("command test temp directory should be created");
        let path = temp_dir.path().join("stdin.txt");
        fs::write(&path, b"stdin-file").expect("stdin fixture should be written");

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

        assert_eq!(output.stdout_text().expect("stdout should be valid UTF-8"), "inherited",);
    }

    #[test]
    fn test_command_runner_run_reports_missing_stdin_file() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-test-").expect("command test temp directory should be created");
        let path = temp_dir.path().join("missing-stdin.txt");
        let error = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("cat").stdin_file(path.clone()))
            .expect_err("missing stdin file should be reported");

        match error.reason() {
            CommandErrorReason::OpenInputFailed { path: actual_path, .. } => {
                assert_eq!(actual_path, &path)
            }
            other => panic!("expected stdin open failure, got {other:?}"),
        }
    }

    #[test]
    fn test_command_runner_disable_logging_updates_configuration() {
        let runner = CommandRunner::new(Duration::from_secs(10)).disable_logging(true);

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
        let runner = CommandRunner::new(Duration::from_secs(10)).fail_on_output_truncation(false);

        assert!(!runner.is_output_truncation_failure_enabled());
    }

    #[test]
    fn test_command_runner_output_file_updates_configuration() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-test-").expect("command test temp directory should be created");
        let stdout_path = temp_dir.path().join("stdout-config.txt");
        let stderr_path = temp_dir.path().join("stderr-config.txt");
        let options = CommandRunOptions::new()
            .tee_stdout_to_file(stdout_path.clone())
            .tee_stderr_to_file(stderr_path.clone());

        assert_eq!(options.configured_stdout_file(), Some(stdout_path.as_path()));
        assert_eq!(options.configured_stderr_file(), Some(stderr_path.as_path()));
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
        assert!(records.iter().any(|(_, message)| message.contains("Running")));
        assert!(records.iter().any(|(_, message)| message.contains("Finished")));
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

        assert_eq!(output.stdout(), format!("{MARKER}--password{SECRET}").as_bytes());

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
                assert!(error.output().expect("timeout output").elapsed() >= Duration::from_millis(50));
            }
            other => panic!("expected timeout error, got {other:?}"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_returns_when_descendant_escapes_process_group() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        run_timeout_fixture_with_watchdog(&pid_path, "stdout");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_cancels_incomplete_stream() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-child.pid");
        run_timeout_fixture_with_watchdog(&pid_path, "stderr");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_cancels_blocked_stdin_writer() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
            .expect("command test temporary directory should be created");
        let pid_path = temp_dir.path().join("escaped-stdin-child.pid");
        run_timeout_fixture_with_watchdog(&pid_path, "stdin");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_ready_timeout_is_bounded_and_cleaned() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-").expect("temporary directory should be created");
        run_timeout_fixture_with_watchdog(&temp_dir.path().join("escaped.pid"), "ready-timeout");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_without_clock_advance_does_not_report_timeout() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-").expect("temporary directory should be created");
        run_timeout_fixture_with_watchdog(&temp_dir.path().join("escaped.pid"), "no-advance");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_worker_panic_is_cleaned() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-").expect("temporary directory should be created");
        run_timeout_fixture_with_watchdog(&temp_dir.path().join("escaped.pid"), "worker-panic");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_command_runner_timeout_watchdog_cleans_stalled_fixture() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-test-").expect("temporary directory should be created");
        run_timeout_fixture_with_watchdog(&temp_dir.path().join("escaped.pid"), "watchdog-self-test");
    }

    #[test]
    fn test_runner_timeout_uses_injected_manual_timer() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-managed-test-").expect("test temp directory should be created");
        let pid_path = temp_dir.path().join("command.pid");
        let test_name = std::thread::current()
            .name()
            .expect("test thread should be named")
            .to_owned();
        super::support::managed_test_process::run(&test_name, &pid_path, || {
            let pid_path = super::support::managed_test_process::command_pid_path();
            let abort_path = super::support::managed_test_process::command_abort_path();
            let abort_ack_path = super::support::managed_test_process::command_abort_ack_path();
            let clock = ManualMonotonicClock::new_shared();
            let runner = CommandRunner::new(Duration::from_secs(30)).timer(clock.new_timer());
            let cancellation = CommandCancellation::new();
            let worker_cancellation = cancellation.clone();
            let worker_pid_path = pid_path.clone();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let worker = std::thread::spawn(move || {
                if !super::support::managed_test_process::permit_command_start() {
                    return;
                }
                let command = Command::new("sh")
                    .arg("-c")
                    .arg(&format!(
                        "{}exec sleep 60",
                        super::support::managed_test_process::shell_start_guard()
                    ))
                    .arg("sh")
                    .arg_os(&worker_pid_path)
                    .arg_os(&abort_path)
                    .arg_os(&abort_ack_path);
                let result = runner.run_with(command, CommandRunOptions::new().cancellation(worker_cancellation));
                let _ = sender.send(result);
            });

            let waiting = clock.wait_for_waiters(1, Duration::from_secs(5));
            let pid_deadline = Instant::now() + Duration::from_secs(5);
            while !pid_path.exists() && Instant::now() < pid_deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let pid_ready = pid_path.exists();
            if waiting && pid_ready {
                clock
                    .advance(Duration::from_secs(30))
                    .expect("manual time should advance");
            } else {
                cancellation.cancel();
            }
            let result = receiver.recv_timeout(Duration::from_secs(5));
            if result.is_err() {
                cancellation.cancel();
            }
            let join = worker.join();
            assert!(waiting, "runner should register its manual timeout waiter");
            assert!(pid_ready, "command should publish its PID before manual time advances");
            let error = result
                .expect("runner should return within the real-time watchdog")
                .expect_err("command should fail");
            join.expect("runner thread should not panic");
            assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        });
    }

    #[test]
    fn test_runner_timeout_accepts_child_that_exits_before_deadline() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-managed-test-").expect("test temp directory should be created");
        let pid_path = temp_dir.path().join("command.pid");
        let test_name = std::thread::current()
            .name()
            .expect("test thread should be named")
            .to_owned();
        super::support::managed_test_process::run(&test_name, &pid_path, || {
            let temp_dir = LocalTempDir::with_prefix("qubit-command-test-")
                .expect("command test temp directory should be created");
            let signal_path = temp_dir.path().join("deadline-signal");
            let completion_path = temp_dir.path().join("deadline-completion");
            let clock = ManualMonotonicClock::new_shared();
            let timeout = Duration::from_secs(30);
            let runner = CommandRunner::new(timeout).timer(clock.new_timer());
            let cancellation = CommandCancellation::new();
            let worker_cancellation = cancellation.clone();
            let pid_path = super::support::managed_test_process::command_pid_path();
            let abort_path = super::support::managed_test_process::command_abort_path();
            let abort_ack_path = super::support::managed_test_process::command_abort_ack_path();
            let worker_pid_path = pid_path.clone();
            let child_signal_path = signal_path.clone();
            let child_completion_path = completion_path.clone();
            let worker = std::thread::spawn(move || {
                if !super::support::managed_test_process::permit_command_start() {
                    loop {
                        std::thread::sleep(Duration::from_secs(60));
                    }
                }
                let script = format!(
                    "{}while [ ! -e \"$4\" ]; do sleep 0.01; done; : > \"$5\"",
                    super::support::managed_test_process::shell_start_guard()
                );
                runner.run_with(
                    Command::new("sh")
                        .arg("-c")
                        .arg(&script)
                        .arg("sh")
                        .arg_os(&worker_pid_path)
                        .arg_os(&abort_path)
                        .arg_os(&abort_ack_path)
                        .arg_os(&child_signal_path)
                        .arg_os(&child_completion_path),
                    CommandRunOptions::new().cancellation(worker_cancellation),
                )
            });

            let waiting = clock.wait_for_waiters(1, Duration::from_secs(5));
            let pid_deadline = Instant::now() + Duration::from_secs(5);
            while !pid_path.exists() && Instant::now() < pid_deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let pid_ready = pid_path.exists();
            if waiting && pid_ready {
                fs::write(&signal_path, b"release").expect("signal file should release child command");
            } else {
                cancellation.cancel();
            }
            let completed_before_deadline = Instant::now() + Duration::from_secs(2);
            let mut completed = completion_path.exists();
            while waiting && pid_ready && !completed && Instant::now() < completed_before_deadline {
                std::thread::sleep(Duration::from_millis(10));
                completed = completion_path.exists();
            }
            // The completion marker is written immediately before the shell exits,
            // so keep advancing manual time in small steps until the runner has
            // observed the exit. Advancing the full timeout here can race with the
            // child process scheduler and turn a successful command into a timeout.
            let observed_exit_before_deadline = Instant::now() + Duration::from_secs(2);
            while waiting
                && pid_ready
                && completed
                && !worker.is_finished()
                && Instant::now() < observed_exit_before_deadline
            {
                clock
                    .advance(Duration::from_millis(1))
                    .expect("manual time should advance");
                std::thread::sleep(Duration::from_millis(1));
            }

            let successful_result = worker.is_finished();
            if !successful_result {
                cancellation.cancel();
            }
            let result = worker.join();
            assert!(waiting, "runner should register its manual timeout waiter");
            assert!(pid_ready, "command should publish its PID before release");
            assert!(
                completed,
                "child command should complete before the manual deadline advances"
            );
            assert!(
                successful_result,
                "runner should observe the completed child before timeout"
            );
            let _ = result
                .expect("runner thread should not panic")
                .expect("a child that exits before the deadline should complete normally");
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_managed_test_process_watchdog_self_test() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-watchdog-self-test-")
            .expect("self-test temp directory should be created");
        let pid_path = temp_dir.path().join("command.pid");
        let test_name = std::thread::current()
            .name()
            .expect("test thread should be named")
            .to_owned();
        let result =
            super::support::managed_test_process::run_for(&test_name, Some(&pid_path), Duration::from_secs(1), || {
                let command_pid_path = super::support::managed_test_process::command_pid_path();
                let abort_path = super::support::managed_test_process::command_abort_path();
                let abort_ack_path = super::support::managed_test_process::command_abort_ack_path();
                let worker = std::thread::spawn(move || {
                    if !super::support::managed_test_process::permit_command_start() {
                        loop {
                            std::thread::sleep(Duration::from_secs(60));
                        }
                    }
                    let script = format!(
                        "sleep 2; {}exec sleep 60",
                        super::support::managed_test_process::shell_start_guard()
                    );
                    CommandRunner::new(Duration::from_secs(60)).run(
                        Command::new("sh")
                            .arg("-c")
                            .arg(&script)
                            .arg("sh")
                            .arg_os(&command_pid_path)
                            .arg_os(&abort_path)
                            .arg_os(&abort_ack_path),
                    )
                });
                let command_pid_path = super::support::managed_test_process::command_pid_path();
                let deadline = Instant::now() + Duration::from_secs(5);
                while !command_pid_path.exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                }
                // This fixture intentionally delays PID publication until after
                // the short watchdog requests abort.
                let _worker_is_intentionally_live = worker;
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                }
            });
        assert!(
            result
                .as_ref()
                .is_err_and(|error| error.contains("exceeded 1s watchdog")),
            "watchdog should wait for late PID publication, abort, kill, and reap: {result:?}",
        );
        drop(temp_dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_managed_test_process_readiness_failure_cleans_runner() {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-readiness-failure-")
            .expect("test temp directory should be created");
        let pid_path = temp_dir.path().join("command.pid");
        let test_name = std::thread::current()
            .name()
            .expect("test thread should be named")
            .to_owned();
        super::support::managed_test_process::run(&test_name, &pid_path, || {
            let command_pid_path = super::support::managed_test_process::command_pid_path();
            let abort_path = super::support::managed_test_process::command_abort_path();
            let abort_ack_path = super::support::managed_test_process::command_abort_ack_path();
            let cancellation = CommandCancellation::new();
            let worker_cancellation = cancellation.clone();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let worker = std::thread::spawn(move || {
                if !super::support::managed_test_process::permit_command_start() {
                    return;
                }
                let result = CommandRunner::without_timeout().run_with(
                    Command::new("sh")
                        .arg("-c")
                        .arg(&format!(
                            "{}exec sleep 60",
                            super::support::managed_test_process::shell_start_guard()
                        ))
                        .arg("sh")
                        .arg_os(&command_pid_path)
                        .arg_os(&abort_path)
                        .arg_os(&abort_ack_path),
                    CommandRunOptions::new().cancellation(worker_cancellation),
                );
                let _ = sender.send(result);
            });
            let publish_deadline = Instant::now() + Duration::from_secs(5);
            while !super::support::managed_test_process::command_pid_path().exists()
                && Instant::now() < publish_deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                super::support::managed_test_process::command_pid_path().exists(),
                "stalled command should publish PID before readiness is observed",
            );
            let readiness_deadline = Instant::now() + Duration::from_millis(150);
            let ready_marker = temp_dir.path().join("never-ready");
            while !ready_marker.exists() && Instant::now() < readiness_deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let ready = ready_marker.exists();
            cancellation.cancel();
            let result = receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("cancelled runner should return after failed readiness");
            worker
                .join()
                .expect("runner worker should be joined after failed readiness");
            assert!(!ready, "fixture intentionally withholds its readiness marker");
            assert_eq!(
                result.expect_err("failed readiness path should cancel runner").kind(),
                CommandErrorKind::Cancelled
            );
        });
    }

    #[test]
    fn test_command_runner_timer_updates_configuration() {
        let clock = ManualMonotonicClock::new_shared();
        let runner = CommandRunner::new(Duration::from_secs(10)).timer(clock.new_timer());

        assert_eq!(runner.configured_timer().clock().now().domain(), clock.now().domain(),);
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
    fn test_command_runner_run_times_out_when_background_child_inherits_output() {
        let start = Instant::now();
        let error = CommandRunner::new(Duration::from_millis(50))
            .run(Command::shell("sleep 2 &"))
            .expect_err("background child with inherited output pipes should time out");

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
        let output = error.output().expect("truncation error should expose output");
        assert_eq!(output.stdout(), b"abc");
        assert_eq!(output.stderr(), b"wx");
        assert!(output.stdout_truncated());
        assert!(output.stderr_truncated());
    }

    #[test]
    fn test_command_runner_bounded_output_limits_streams_and_rejects_truncation() {
        let runner = CommandRunner::new(Duration::from_secs(10)).bounded_output(3);

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
        assert!(matches!(
            error.reason(),
            CommandErrorReason::UnexpectedExit {
                exit_code: Some(7),
                expected,
            } if expected == &[0]
        ));
        assert!(
            error
                .output()
                .expect("unexpected exit should expose output")
                .stdout_truncated()
        );
        assert!(error.cleanup_failures().is_empty());
    }

    #[test]
    fn test_command_runner_run_tees_output_to_files() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-test-").expect("command test temp directory should be created");
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
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-test-").expect("command test temp directory should be created");
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
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-test-").expect("command test temp directory should be created");
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
            .run(Command::new("__qubit_command_missing_executable__").arg("two words"))
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
            .run(Command::new("__qubit_command_missing_executable__").arg("-Dpassword=jvm-secret"))
            .expect_err("missing executable should fail to spawn");

        assert_eq!(
            error.command(),
            r#"["__qubit_command_missing_executable__", "-Dpassword=<redacted>"]"#,
        );
        assert!(!error.command().contains("jvm-secret"));
    }

    #[test]
    fn test_command_runner_error_masks_sensitive_option_after_double_dash() {
        let error = CommandRunner::new(Duration::from_secs(10))
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
        let policy = RedactionPolicy::default()
            .to_builder()
            .fields(|fields| {
                fields.raise("tenant_option", Sensitivity::Secret);
            })
            .expect("the test policy field must be valid")
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
    fn test_command_runner_error_redacts_multiple_configured_sensitive_fields() {
        let policy = RedactionPolicy::default()
            .to_builder()
            .fields(|fields| {
                fields
                    .raise("tenant_option", Sensitivity::Secret)
                    .raise("tenant_env", Sensitivity::Secret);
            })
            .expect("the test policy field must be valid")
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
    fn test_command_runner_floor_overrides_exact_allow_for_default_sensitive_fields() {
        let policy = RedactionPolicy::default()
            .to_builder()
            .fields(|fields| {
                fields.allow_exact("sig").allow_exact("signature");
            })
            .expect("the test policy field must be valid")
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
    fn test_command_runner_floor_overrides_suffix_allow_for_default_sensitive_fields() {
        let policy = RedactionPolicy::default()
            .to_builder()
            .fields(|fields| {
                fields.allow_suffix("access_token");
            })
            .expect("the test policy field must be valid")
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

#[test]
fn test_command_runner_tee_configurations_keep_streams_separate() {
    for configuration in ["none", "stdout", "stderr", "both"] {
        let temp_dir = LocalTempDir::with_prefix("qubit-command-tee-config-")
            .expect("command test temp directory should be created");
        let stdout_path = temp_dir.path().join("stdout.txt");
        let stderr_path = temp_dir.path().join("stderr.txt");
        let mut options = CommandRunOptions::new();
        if matches!(configuration, "stdout" | "both") {
            options = options.tee_stdout_to_file(&stdout_path);
        }
        if matches!(configuration, "stderr" | "both") {
            options = options.tee_stderr_to_file(&stderr_path);
        }

        let output = CommandRunner::new(Duration::from_secs(10))
            .run_with(
                support::output_command::output_command(b"stdout-payload", b"stderr-payload", 0),
                options,
            )
            .expect("command should run successfully");

        assert_eq!(output.stdout(), b"stdout-payload");
        assert_eq!(output.stderr(), b"stderr-payload");
        if matches!(configuration, "stdout" | "both") {
            assert_eq!(
                fs::read(&stdout_path).expect("stdout tee should exist"),
                b"stdout-payload"
            );
        } else {
            assert!(!stdout_path.exists());
        }
        if matches!(configuration, "stderr" | "both") {
            assert_eq!(
                fs::read(&stderr_path).expect("stderr tee should exist"),
                b"stderr-payload"
            );
        } else {
            assert!(!stderr_path.exists());
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::thread;

    use super::Command;
    use super::CommandCancellation;
    use super::CommandErrorKind;
    use super::CommandRunOptions;
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
            trim_windows_line_endings(output.stdout_text().expect("stdout should be UTF-8")),
            "command-out",
        );
    }

    #[test]
    fn test_command_runner_windows_captures_stderr() {
        let output = CommandRunner::new(Duration::from_secs(10))
            .run(Command::shell("echo command-error>&2"))
            .expect("Windows shell command should run successfully");

        assert_eq!(
            trim_windows_line_endings(output.stderr_text().expect("stderr should be UTF-8")),
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
        canceller.join().expect("Windows cancellation thread should finish");

        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancellation should not wait for the command to exit normally",
        );
    }

    #[test]
    fn test_command_runner_windows_times_out_when_background_child_inherits_output() {
        let started = Instant::now();
        let error = CommandRunner::new(Duration::from_millis(250))
            .run(Command::shell("start \"\" /B ping -n 6 127.0.0.1"))
            .expect_err("background child with inherited output should time out");

        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout should not wait for the background child to exit",
        );
    }

    #[test]
    fn test_command_runner_windows_tees_output_to_file() {
        let temp_dir =
            LocalTempDir::with_prefix("qubit-command-windows-").expect("command test temp directory should be created");
        let stdout_path = temp_dir.path().join("stdout.txt");
        let output = CommandRunner::new(Duration::from_secs(10))
            .max_stdout_bytes(3)
            .fail_on_output_truncation(false)
            .run_with(
                Command::shell("echo abcdef"),
                CommandRunOptions::new().tee_stdout_to_file(stdout_path.clone()),
            )
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
