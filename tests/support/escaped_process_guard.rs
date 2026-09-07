use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) struct EscapedProcessGuard {
    pid_path: PathBuf,
}

impl EscapedProcessGuard {
    pub(crate) fn new(pid_path: PathBuf) -> Self {
        Self { pid_path }
    }

    pub(crate) fn wait_until_recorded(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.pid_path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            self.pid_path.exists(),
            "escaped descendant must record its PID"
        );
    }

    pub(crate) fn terminate_and_wait(&mut self) {
        self.terminate_and_wait_inner()
            .expect("escaped descendant must be terminated");
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
        Err(format!(
            "escaped descendant {pid} still exists after 1 second"
        ))
    }
}

impl Drop for EscapedProcessGuard {
    fn drop(&mut self) {
        let _ = self.terminate_and_wait_inner();
    }
}
