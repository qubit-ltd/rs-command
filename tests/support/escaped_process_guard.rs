// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

pub(crate) struct EscapedProcessGuard {
    pid_path: PathBuf,
    armed: bool,
}

impl EscapedProcessGuard {
    pub(crate) fn new(pid_path: PathBuf) -> Self {
        Self { pid_path, armed: true }
    }

    pub(crate) fn wait_until_recorded(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(contents) = fs::read_to_string(&self.pid_path)
                && contents.trim().parse::<u32>().is_ok_and(|pid| pid != 0)
            {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("escaped descendant must record a non-zero PID");
    }

    pub(crate) fn terminate_and_wait(&mut self) {
        self.terminate_and_wait_inner()
            .expect("escaped descendant must be terminated");
        self.armed = false;
    }

    fn terminate_and_wait_inner(&self) -> Result<(), String> {
        let pid = fs::read_to_string(&self.pid_path)
            .map_err(|error| format!("read escaped descendant PID: {error}"))?
            .trim()
            .parse::<u32>()
            .map_err(|error| format!("parse escaped descendant PID: {error}"))?;
        let pid = pid.to_string();
        let killed = Command::new("kill")
            .args(["-KILL", &pid])
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("kill escaped descendant: {error}"))?;
        if !killed.success() {
            return Err(format!("kill -KILL {pid} exited with {killed}"));
        }

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            let still_exists = Command::new("kill")
                .args(["-0", &pid])
                .stderr(Stdio::null())
                .status()
                .map_err(|error| format!("probe escaped descendant: {error}"))?
                .success();
            if !still_exists {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(format!("escaped descendant {pid} still exists after 1 second"))
    }
}

impl Drop for EscapedProcessGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.terminate_and_wait_inner();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    use super::EscapedProcessGuard;

    #[test]
    fn terminate_disarms_guard_after_success() {
        let pid_path = std::env::temp_dir().join(format!("qubit-escaped-process-guard-{}.pid", std::process::id()));
        let mut child = Command::new("sleep").arg("10").spawn().expect("sleep should start");
        let child_pid = child.id();
        fs::write(&pid_path, child_pid.to_string()).expect("PID file should be written");
        let waiter = thread::spawn(move || child.wait().expect("sleep should be reaped"));

        let mut guard = EscapedProcessGuard::new(pid_path.clone());
        guard.terminate_and_wait();
        assert!(!guard.armed, "successful cleanup must disarm the guard");
        waiter.join().expect("sleep waiter should finish");
        let _ = fs::remove_file(pid_path);
    }

    #[test]
    fn wait_until_recorded_ignores_empty_pid_file() {
        let pid_path =
            std::env::temp_dir().join(format!("qubit-escaped-process-guard-empty-{}.pid", std::process::id()));
        fs::write(&pid_path, "").expect("empty PID file should be written");
        let path_for_writer = pid_path.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            fs::write(path_for_writer, "1234").expect("PID should be recorded");
        });

        let guard = EscapedProcessGuard::new(pid_path.clone());
        guard.wait_until_recorded(Duration::from_secs(1));
        let _ = fs::remove_file(pid_path);
    }
}
