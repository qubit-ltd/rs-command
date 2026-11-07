// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix subprocess watchdogs for tests that exercise real commands.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use super::temp_dir::LocalTempDir;

const CHILD_ENV: &str = "QUBIT_COMMAND_MANAGED_TEST_CHILD";
const PID_ENV: &str = "QUBIT_COMMAND_MANAGED_TEST_PID";
const GATE_ENV: &str = "QUBIT_COMMAND_MANAGED_TEST_GATE";
const ABORT_ENV: &str = "QUBIT_COMMAND_MANAGED_TEST_ABORT";
const ACK_ENV: &str = "QUBIT_COMMAND_MANAGED_TEST_ACK";

/// Runs the current test in an isolated subprocess with a real-time deadline.
///
/// The test body publishes the runner command PID at `pid_path` immediately
/// before doing work, allowing the parent to clean up the runner's process
/// group if the test process exceeds its deadline.
pub(crate) fn run(test_name: &str, pid_path: &std::path::Path, body: impl FnOnce()) {
    run_for(test_name, Some(pid_path), Duration::from_secs(30), body)
        .expect("managed test subprocess should finish before its watchdog");
}

/// Runs the current test with an explicit watchdog deadline.
///
/// `pid_path` is `None` only for watchdog self-tests that do not start a
/// CommandRunner process group.
pub(crate) fn run_for(
    test_name: &str,
    pid_path: Option<&std::path::Path>,
    watchdog: Duration,
    body: impl FnOnce(),
) -> Result<(), String> {
    run_for_with_env(test_name, pid_path, watchdog, &[], body)
}

/// Runs a managed test with additional environment variables set only on its
/// child.
pub(crate) fn run_for_with_env(
    test_name: &str,
    pid_path: Option<&std::path::Path>,
    watchdog: Duration,
    extra_env: &[(&OsStr, &OsStr)],
    body: impl FnOnce(),
) -> Result<(), String> {
    if std::env::var_os(CHILD_ENV).is_some() {
        let gate = std::env::var_os(GATE_ENV).expect("managed test parent should provide a start gate");
        let gate = std::path::PathBuf::from(gate);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(gate.exists(), "parent should release the managed test start gate");
        body();
        return Ok(());
    }

    let gate_directory = LocalTempDir::with_prefix("qubit-command-managed-gate-")
        .expect("managed test start gate directory should be created");
    let gate_path = gate_directory.path().join("start");
    let abort_path = pid_path.map(|path| path.with_extension("abort"));
    let ack_path = pid_path.map(|path| path.with_extension("ack"));
    if let Some(path) = &abort_path {
        let _ = fs::remove_file(path);
    }
    if let Some(path) = &ack_path {
        let _ = fs::remove_file(path);
    }
    let mut child = Command::new(std::env::current_exe().expect("test executable should be available"))
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env(PID_ENV, pid_path.unwrap_or_else(|| std::path::Path::new("")))
        .env(GATE_ENV, &gate_path)
        .envs(extra_env.iter().copied())
        .env(ABORT_ENV, abort_path.as_deref().unwrap_or(std::path::Path::new("")))
        .env(ACK_ENV, ack_path.as_deref().unwrap_or(std::path::Path::new("")))
        .process_group(0)
        .spawn()
        .expect("managed test subprocess should start");
    fs::write(&gate_path, b"start").expect("managed test start gate should open");
    let deadline = Instant::now() + watchdog;
    loop {
        match child.try_wait().expect("managed test subprocess should be observable") {
            Some(status) => {
                let pgid = i32::try_from(child.id()).map_err(|error| format!("test PID overflow: {error}"))?;
                let test_group_cleanup = cleanup_unexpected_test_group(pgid);
                let command_group_cleanup = pid_path.map(cleanup_unexpected_command_group).transpose();
                test_group_cleanup?;
                command_group_cleanup?;
                if status.success() {
                    return Ok(());
                }
                return Err(format!("managed test subprocess failed: {status}"));
            }
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            None => {
                let resolution = if let (Some(pid_path), Some(abort_path), Some(ack_path)) =
                    (pid_path, abort_path.as_ref(), ack_path.as_ref())
                {
                    Some(
                        fs::write(abort_path, b"abort")
                            .map_err(|error| format!("request command start abort: {error}"))
                            .and_then(|()| wait_for_spawn_resolution(pid_path, ack_path, Duration::from_secs(3))),
                    )
                } else {
                    None
                };
                let pgid = i32::try_from(child.id()).map_err(|error| format!("test PID overflow: {error}"))?;
                let group_kill = unsafe { libc::kill(-pgid, libc::SIGKILL) };
                if group_kill != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(format!("watchdog should terminate test process group: {error}"));
                    }
                }
                child
                    .wait()
                    .map_err(|error| format!("watchdog should reap test subprocess: {error}"))?;
                wait_for_group_gone(pgid, Duration::from_secs(1))?;
                let mut cleanup_error = None;
                let mut command_group_clean = false;
                if let Some(pid_path) = pid_path {
                    if pid_path.exists() {
                        terminate_command_group(pid_path)?;
                        command_group_clean = true;
                    }
                    if matches!(resolution, Some(Ok(SpawnResolution::NotStarted))) {
                        if pid_path.exists() {
                            return Err("command PID appeared after not-started acknowledgement".to_owned());
                        }
                        command_group_clean = true;
                    }
                    if !command_group_clean {
                        let published = wait_for_pid_publication(pid_path, Duration::from_secs(3));
                        if published {
                            terminate_command_group(pid_path)?;
                            command_group_clean = true;
                        }
                    }
                    if let Some(Err(error)) = &resolution {
                        cleanup_error = Some(error.clone());
                    }
                    if !command_group_clean {
                        return Err(
                            cleanup_error.unwrap_or_else(|| "command process cleanup was not acknowledged".to_owned())
                        );
                    }
                } else {
                    command_group_clean = true;
                }
                match resolution {
                    Some(Ok(SpawnResolution::Published)) if pid_path.is_some_and(|p| p.exists()) => {}
                    Some(Ok(SpawnResolution::NotStarted)) if pid_path.is_some_and(|p| !p.exists()) => {}
                    Some(Err(error)) if command_group_clean => {
                        return Err(format!("spawn cleanup handshake failed: {error}"));
                    }
                    _ => {
                        return Err("spawn cleanup protocol returned an inconsistent state".to_owned());
                    }
                }
                return Err(format!("managed test subprocess exceeded {watchdog:?} watchdog"));
            }
        }
    }
}

fn cleanup_unexpected_test_group(pgid: i32) -> Result<(), String> {
    let result = unsafe { libc::kill(-pgid, 0) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(format!("check test process group {pgid}: {error}"));
    }

    let killed = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    if killed != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(format!("terminate leftover test process group {pgid}: {error}"));
        }
    }
    wait_for_group_gone(pgid, Duration::from_secs(1))?;
    Err(format!(
        "test process group {pgid} had surviving descendants; the group was terminated"
    ))
}

#[derive(Clone, Copy)]
enum SpawnResolution {
    Published,
    NotStarted,
}

fn wait_for_spawn_resolution(
    pid_path: &std::path::Path,
    ack_path: &std::path::Path,
    timeout: Duration,
) -> Result<SpawnResolution, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if pid_path.exists() {
            return Ok(SpawnResolution::Published);
        }
        if let Ok(ack) = fs::read_to_string(ack_path) {
            if ack.trim() == "not-started" {
                return Ok(SpawnResolution::NotStarted);
            }
            return Err(format!("unexpected command abort acknowledgement: {ack:?}"));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "command PID or explicit not-started acknowledgement was not published within {timeout:?}"
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_pid_publication(pid_path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !pid_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    pid_path.exists()
}

fn cleanup_unexpected_command_group(pid_path: &std::path::Path) -> Result<(), String> {
    let value = fs::read_to_string(pid_path).map_err(|error| format!("read command PID after test: {error}"))?;
    let pid = value
        .trim()
        .parse::<i32>()
        .map_err(|error| format!("parse command PID: {error}"))?;
    let result = unsafe { libc::kill(-pid, 0) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(format!("check command process group {pid}: {error}"));
    }

    terminate_command_group(pid_path)?;
    Err(format!(
        "command process group {pid} survived test exit; the group was terminated"
    ))
}

fn wait_for_group_gone(pgid: i32, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let result = unsafe { libc::kill(-pgid, 0) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            // macOS can recycle a just-reaped PGID before this probe; EPERM
            // then means the new group is inaccessible and is not the group
            // this watchdog created.
            if matches!(error.raw_os_error(), Some(libc::ESRCH | libc::EPERM)) {
                return Ok(());
            }
            return Err(format!("check process group {pgid} after termination: {error}"));
        }
        if Instant::now() >= deadline {
            return Err(format!("process group {pgid} remained after {timeout:?}"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Returns the path where the command shell must atomically publish its PID.
pub(crate) fn command_pid_path() -> std::path::PathBuf {
    std::env::var_os(PID_ENV)
        .map(std::path::PathBuf::from)
        .expect("managed test parent should provide a PID handshake path")
}

pub(crate) fn command_abort_path() -> std::path::PathBuf {
    std::env::var_os(ABORT_ENV)
        .map(std::path::PathBuf::from)
        .expect("managed test parent should provide an abort path")
}

pub(crate) fn command_abort_ack_path() -> std::path::PathBuf {
    std::env::var_os(ACK_ENV)
        .map(std::path::PathBuf::from)
        .expect("managed test parent should provide an abort acknowledgement path")
}

/// Records that no runner command will be started after an abort request.
pub(crate) fn permit_command_start() -> bool {
    let abort = command_abort_path();
    if abort.exists() {
        let ack = command_abort_ack_path();
        let temporary = ack.with_extension("ack.tmp");
        fs::write(&temporary, "not-started").expect("not-started acknowledgement should be staged");
        fs::rename(temporary, ack).expect("not-started acknowledgement should be published");
        return false;
    }
    true
}

/// Shell prelude that publishes the command group ID before checking abort.
pub(crate) const fn shell_start_guard() -> &'static str {
    // Keep the group alive briefly after observing abort so the watchdog can
    // signal it before the process group ID can be recycled by the OS.
    "printf '%s\\n' \"$$\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; if [ -e \"$2\" ]; then sleep 5; exit 0; fi; "
}

fn terminate_command_group(pid_path: &std::path::Path) -> Result<(), String> {
    let pid = match fs::read_to_string(pid_path) {
        Ok(value) => value
            .trim()
            .parse::<i32>()
            .map_err(|error| format!("parse command PID: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("command PID was not published; cleanup cannot be verified".to_owned());
        }
        Err(error) => return Err(format!("read command PID: {error}")),
    };
    // CommandRunner uses a dedicated process group on Unix. Negative PID
    // targets that group and cannot include this test's own process group.
    let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
    if result == 0 {
        wait_for_group_gone(pid, Duration::from_secs(1))
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(format!("kill command process group {pid}: {error}"))
        }
    }
}
