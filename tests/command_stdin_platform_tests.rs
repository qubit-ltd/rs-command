// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cross-platform stdin regression tests using this test executable as a child.

use std::fs;
use std::io::Read;
use std::process::Command as ProcessCommand;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use qubit_clock::ManualMonotonicClock;
use qubit_clock::MonotonicClock;
use qubit_command::Command;
use qubit_command::CommandCancellation;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
use tempfile::tempdir;

const HELPER_MARKER: &str = "QUBIT_COMMAND_STDIN_HELPER";
const HELPER_DIR: &str = "QUBIT_COMMAND_STDIN_HELPER_DIR";
const WATCHDOG: Duration = Duration::from_secs(10);

#[test]
fn test_stdin_platform_helper() {
    let Ok(mode) = std::env::var(HELPER_MARKER) else {
        return;
    };
    let directory = std::env::var_os(HELPER_DIR).expect("helper directory should be supplied");
    let directory = std::path::PathBuf::from(directory);
    fs::write(directory.join("pid"), std::process::id().to_string()).expect("helper pid should be recorded");
    match mode.as_str() {
        "receive" => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .expect("stdin should reach EOF");
            let summary = format!("{}:{}", bytes.len(), checksum(&bytes));
            fs::write(directory.join("result"), summary).expect("receive summary should be written");
        }
        "hold" => {
            fs::write(directory.join("ready"), b"ready").expect("ready marker should be written");
            loop {
                thread::park_timeout(Duration::from_secs(60));
            }
        }
        "gate-hold" => {
            fs::write(directory.join("gate"), b"entered").expect("gate marker should be written");
            let deadline = Instant::now() + WATCHDOG * 2;
            while !directory.join("release").exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if directory.join("release").exists() {
                fs::write(directory.join("ready"), b"ready").expect("ready marker should be written");
                loop {
                    thread::park_timeout(Duration::from_secs(60));
                }
            }
        }
        "early-0" | "early-7" => {
            let mut first = [0];
            let _ = std::io::stdin().read(&mut first);
            fs::write(directory.join("first-byte"), first).expect("first byte should be recorded");
            fs::write(directory.join("ack"), b"ack").expect("early-exit acknowledgement should be written");
            if mode == "early-7" {
                std::process::exit(7);
            }
        }
        other => panic!("unknown helper mode: {other}"),
    }
}

#[test]
fn test_stdin_receive_transmits_non_constant_data_and_eof() {
    let directory = tempdir().expect("temporary directory should be created");
    let data = payload(256 * 1024 + 137);
    let result = run_helper("receive", directory.path(), data.clone(), Duration::from_secs(8), None);
    let output = result.expect("receive helper should finish successfully");
    assert_eq!(output.exit_code(), Some(0));
    let expected = format!("{}:{}", data.len(), checksum(&data));
    assert_eq!(
        fs::read_to_string(directory.path().join("result")).expect("summary should exist"),
        expected
    );
}

#[test]
fn test_stdin_hold_is_cancelled_within_watchdog() {
    let directory = tempdir().expect("temporary directory should be created");
    let cancellation = CommandCancellation::new();
    let run_cancellation = cancellation.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let directory_path = directory.path().to_path_buf();
    let worker = thread::spawn(move || {
        let result = run_helper(
            "hold",
            &directory_path,
            payload(4 * 1024 * 1024),
            WATCHDOG,
            Some(run_cancellation),
        );
        let _ = sender.send(result);
    });
    wait_for_file(&directory.path().join("ready"), &directory.path().join("pid"), WATCHDOG);
    cancellation.cancel();
    let result = receive_with_watchdog(receiver, &directory.path().join("pid"));
    worker.join().expect("runner thread should not panic");
    let error = result.expect_err("hold helper should be cancelled");
    assert_eq!(error.kind(), CommandErrorKind::Cancelled);
    assert!(
        error.cleanup_failures().is_empty(),
        "normal cancellation should not report cleanup failures"
    );
}

#[test]
fn test_stdin_hold_times_out_within_watchdog() {
    let directory = tempdir().expect("temporary directory should be created");
    let clock = ManualMonotonicClock::new_shared();
    let timeout = Duration::from_millis(300);
    let runner = CommandRunner::new(timeout).timer(clock.new_timer());
    let (sender, receiver) = std::sync::mpsc::channel();
    let directory_path = directory.path().to_path_buf();
    let rescue_cancellation = CommandCancellation::new();
    let run_cancellation = rescue_cancellation.clone();
    let worker_clock = Arc::clone(&clock);
    let worker = thread::spawn(move || {
        let result = run_helper_with_runner(
            "hold",
            &directory_path,
            payload(4 * 1024 * 1024),
            runner,
            Some(run_cancellation),
        );
        let _ = sender.send(result);
    });
    let ready = wait_for_file_result(&directory.path().join("ready"), &directory.path().join("pid"), WATCHDOG);
    let waiter_ready = ready && clock.wait_for_waiters(1, WATCHDOG);
    if waiter_ready {
        clock.advance(timeout).expect("manual time should advance");
    }
    let result = receive_with_rescue(
        receiver,
        &directory.path().join("pid"),
        &rescue_cancellation,
        &worker_clock,
        timeout,
    );
    worker.join().expect("runner thread should not panic");
    assert!(ready, "hold helper did not become ready before watchdog");
    assert!(waiter_ready, "timeout waiter did not register before watchdog");
    let error = result.expect_err("hold helper should time out");
    assert_eq!(error.kind(), CommandErrorKind::TimedOut);
    assert!(
        error.cleanup_failures().is_empty(),
        "normal timeout should not report cleanup failures"
    );
}

#[test]
fn test_stdin_delayed_ready_does_not_consume_timeout() {
    let directory = tempdir().expect("temporary directory should be created");
    let clock = ManualMonotonicClock::new_shared();
    let timeout = Duration::from_millis(300);
    let runner = CommandRunner::new(timeout).timer(clock.new_timer());
    let cancellation = CommandCancellation::new();
    let worker_cancellation = cancellation.clone();
    let worker_clock = Arc::clone(&clock);
    let directory_path = directory.path().to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let result = run_helper_with_runner(
            "gate-hold",
            &directory_path,
            payload(4 * 1024 * 1024),
            runner,
            Some(worker_cancellation),
        );
        let _ = sender.send(result);
    });

    let gate_seen = wait_for_file_result(&directory.path().join("gate"), &directory.path().join("pid"), WATCHDOG);
    let remained_running = gate_seen && receiver.recv_timeout(Duration::from_millis(400)).is_err();
    let _ = fs::write(directory.path().join("release"), b"release");
    let ready = wait_for_file_result(&directory.path().join("ready"), &directory.path().join("pid"), WATCHDOG);
    let waiter_ready = ready && clock.wait_for_waiters(1, WATCHDOG);
    if waiter_ready {
        clock.advance(timeout).expect("manual time should advance");
    }
    let result = receive_with_rescue(
        receiver,
        &directory.path().join("pid"),
        &cancellation,
        &worker_clock,
        timeout,
    );
    worker.join().expect("runner thread should not panic");
    assert!(gate_seen, "gate helper did not enter before watchdog");
    assert!(remained_running, "runner returned during delayed helper startup");
    assert!(ready, "gate helper did not become ready before watchdog");
    assert!(waiter_ready, "timeout waiter did not register before watchdog");
    let error = result.expect_err("gate helper should time out");
    assert_eq!(error.kind(), CommandErrorKind::TimedOut);
    assert!(
        error.cleanup_failures().is_empty(),
        "normal timeout should not report cleanup failures"
    );
}

#[test]
fn test_stdin_early_exit_does_not_mask_exit_status_with_broken_pipe() {
    for (mode, expected_code) in [("early-0", 0), ("early-7", 7)] {
        let directory = tempdir().expect("temporary directory should be created");
        let (sender, receiver) = std::sync::mpsc::channel();
        let directory_path = directory.path().to_path_buf();
        let mode = mode.to_owned();
        let worker = thread::spawn(move || {
            let result = run_helper(&mode, &directory_path, payload(8 * 1024 * 1024), WATCHDOG, None);
            let _ = sender.send(result);
        });
        let result = receive_with_watchdog(receiver, &directory.path().join("pid"));
        worker.join().expect("runner thread should not panic");
        assert!(
            directory.path().join("ack").exists(),
            "helper should acknowledge the first input byte"
        );
        assert_eq!(
            fs::read(directory.path().join("first-byte")).expect("first byte should exist"),
            [0x5a]
        );
        match expected_code {
            0 => {
                let output = result.expect("successful exit should not be hidden by broken pipe");
                assert_eq!(output.exit_code(), Some(0));
            }
            7 => {
                let error = result.expect_err("nonzero exit should remain visible after broken pipe");
                assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
                assert_eq!(error.exit_code(), Some(7));
                assert!(error.cleanup_failures().is_empty());
            }
            _ => unreachable!(),
        }
    }
}

fn run_helper(
    mode: &str,
    directory: &std::path::Path,
    bytes: Vec<u8>,
    timeout: Duration,
    cancellation: Option<CommandCancellation>,
) -> Result<qubit_command::CommandOutput, qubit_command::CommandError> {
    let runner = CommandRunner::new(timeout);
    run_helper_with_runner(mode, directory, bytes, runner, cancellation)
}

fn run_helper_with_runner(
    mode: &str,
    directory: &std::path::Path,
    bytes: Vec<u8>,
    runner: CommandRunner,
    cancellation: Option<CommandCancellation>,
) -> Result<qubit_command::CommandOutput, qubit_command::CommandError> {
    let executable = std::env::current_exe().expect("test executable path should be available");
    let executable = executable.to_str().expect("test executable path should be UTF-8");
    let mut command = Command::new(executable);
    command = command
        .arg("--exact")
        .arg("test_stdin_platform_helper")
        .arg("--nocapture");
    command = command
        .env(HELPER_MARKER, mode)
        .env(
            HELPER_DIR,
            directory.to_str().expect("temporary directory path should be UTF-8"),
        )
        .stdin_bytes(bytes);
    let options = cancellation.map_or_else(CommandRunOptions::new, |token| {
        CommandRunOptions::new().cancellation(token)
    });
    runner.run_with(command, options)
}

fn payload(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| (index.wrapping_mul(31) ^ (index >> 7) ^ 0x5a) as u8)
        .collect()
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn wait_for_file(path: &std::path::Path, pid_path: &std::path::Path, timeout: Duration) {
    if !wait_for_file_result(path, pid_path, timeout) {
        terminate_helper(pid_path);
        panic!("helper did not create {} before watchdog", path.display());
    }
}

fn wait_for_file_result(path: &std::path::Path, pid_path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        if pid_path.exists() {
            thread::sleep(Duration::from_millis(10));
        } else {
            thread::yield_now();
        }
    }
    false
}

fn receive_with_rescue<T>(
    receiver: std::sync::mpsc::Receiver<T>,
    pid_path: &std::path::Path,
    cancellation: &CommandCancellation,
    clock: &ManualMonotonicClock,
    timeout: Duration,
) -> T {
    match receiver.recv_timeout(WATCHDOG) {
        Ok(result) => result,
        Err(error) => {
            cancellation.cancel();
            let _ = clock.advance(timeout);
            terminate_helper(pid_path);
            receiver
                .recv_timeout(WATCHDOG)
                .unwrap_or_else(|_| panic!("runner did not finish after rescue; initial wait error: {error}"))
        }
    }
}

fn receive_with_watchdog<T>(receiver: std::sync::mpsc::Receiver<T>, pid_path: &std::path::Path) -> T {
    match receiver.recv_timeout(WATCHDOG) {
        Ok(result) => result,
        Err(error) => {
            terminate_helper(pid_path);
            panic!("stdin runner exceeded {WATCHDOG:?} watchdog: {error}");
        }
    }
}

fn terminate_helper(pid_path: &std::path::Path) {
    let Ok(pid) = fs::read_to_string(pid_path) else {
        return;
    };
    #[cfg(windows)]
    let status = ProcessCommand::new("taskkill")
        .args(["/F", "/PID", pid.trim()])
        .status();
    #[cfg(unix)]
    let status = ProcessCommand::new("kill").args(["-KILL", pid.trim()]).status();
    let _ = status;
}
