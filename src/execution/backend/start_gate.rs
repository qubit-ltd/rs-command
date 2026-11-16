//! Linearizes cancellation against the first operation with external effects.
use crate::CommandCancellation;
use crate::execution::contracts::StopPort;
use qubit_operation::StopReason;
use std::sync::Mutex;

#[derive(Debug, Default)]
pub(crate) struct StartGate {
    state: Mutex<State>,
    cancellation: CommandCancellation,
}

#[derive(Debug, Default)]
struct State {
    stopped: bool,
    authorized: bool,
    external_cancellation: Option<CommandCancellation>,
}

#[derive(Debug)]
pub(crate) struct StartPermit(());

impl StartGate {
    /// Returns a permit only when no stop request has won the start race.
    pub(crate) fn authorize(&self) -> Option<StartPermit> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped || state.authorized {
            return None;
        }
        state.authorized = true;
        Some(StartPermit(()))
    }

    /// Records stop without waiting for the authorized system call to finish.
    pub(crate) fn request_stop(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let won_before_start = !state.authorized;
        state.stopped = true;
        self.cancellation.cancel();
        if let Some(cancellation) = &state.external_cancellation {
            cancellation.cancel();
        }
        won_before_start
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }
}

impl StartGate {
    pub(crate) fn cancellation(&self) -> CommandCancellation {
        self.cancellation.clone()
    }

    pub(crate) fn attach_external_cancellation(&self, cancellation: Option<CommandCancellation>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            if let Some(cancellation) = cancellation {
                cancellation.cancel();
            }
        } else {
            state.external_cancellation = cancellation;
        }
    }
}

impl StopPort for StartGate {
    fn request_stop(&self, _reason: StopReason) {
        let _ = Self::request_stop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::StartGate;
    use crate::{Command, CommandErrorKind, CommandRunOptions, CommandRunner};
    use std::fs;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn stop_wins_start_gate() {
        let gate = StartGate::default();
        assert!(gate.request_stop());
        assert!(gate.authorize().is_none());
    }

    #[test]
    fn side_effect_authorization_wins_stop_gate() {
        let gate = StartGate::default();
        assert!(gate.authorize().is_some());
        assert!(!gate.request_stop());
        assert!(gate.stop_requested());
    }

    #[cfg(unix)]
    #[test]
    fn stop_before_commit_does_not_spawn_or_truncate() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("spawned");
        let tee = directory.path().join("tee");
        let marker_arg = marker.to_string_lossy().into_owned();
        fs::write(&tee, b"keep").unwrap();
        let gate = StartGate::default();
        gate.request_stop();
        let result = CommandRunner::without_timeout().run_with_tree_policy(
            Command::new("sh")
                .arg("-c")
                .arg("touch \"$1\"")
                .arg("managed-test")
                .arg(&marker_arg),
            CommandRunOptions::new()
                .cancellation(gate.cancellation())
                .tee_stdout_to_file(&tee),
            Some(crate::ProcessTreePolicy::Managed),
            Some(&gate),
            None,
        );
        assert_eq!(
            result.unwrap_err().kind(),
            CommandErrorKind::CancelledBeforeStart
        );
        assert!(!marker.exists());
        assert_eq!(fs::read(&tee).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn stop_after_spawn_reaches_child_and_waits_for_runner_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let ready = directory.path().join("ready");
        let ready_arg = ready.to_string_lossy().into_owned();
        let gate = Arc::new(StartGate::default());
        let run_gate = Arc::clone(&gate);
        let runner = thread::spawn(move || {
            CommandRunner::without_timeout().run_with_tree_policy(
                Command::new("sh")
                    .arg("-c")
                    .arg("touch \"$1\"; exec sleep 30")
                    .arg("managed-test")
                    .arg(&ready_arg),
                CommandRunOptions::new().cancellation(run_gate.cancellation()),
                Some(crate::ProcessTreePolicy::Managed),
                Some(&run_gate),
                None,
            )
        });
        let started = Instant::now();
        let ready_path = directory.path().join("ready");
        while !ready_path.exists() && started.elapsed() < Duration::from_secs(5) {
            thread::yield_now();
        }
        assert!(
            ready_path.exists(),
            "child did not reach its readiness marker"
        );
        gate.request_stop();
        let result = runner.join().expect("runner thread should join");
        assert_eq!(result.unwrap_err().kind(), CommandErrorKind::Cancelled);
    }
}
