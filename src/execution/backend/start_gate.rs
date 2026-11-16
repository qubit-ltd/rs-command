// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Linearizes cancellation against the first operation with external effects.
use std::sync::Mutex;

use qubit_operation::StopReason;

use super::internal::StartPermit;
use super::internal::State;
use crate::CommandCancellation;
use crate::execution::contracts::StopPort;

/// Represents the start gate state or contract used by managed command
/// execution.
#[derive(Debug, Default)]
pub(crate) struct StartGate {
    /// Stores the state value used by this execution component.
    state: Mutex<State>,
    /// Stores the cancellation value used by this execution component.
    cancellation: CommandCancellation,
}

impl StartGate {
    /// Returns a permit only when no stop request has won the start race.
    pub(crate) fn authorize(&self) -> Option<StartPermit> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state
            .external_cancellation
            .as_ref()
            .is_some_and(CommandCancellation::is_cancelled)
        {
            state.stopped = true;
            state.reason = Some(StopReason::UserCancelled);
            self.cancellation.cancel();
        }
        if state.stopped {
            return None;
        }
        if state.authorized {
            return Some(StartPermit(()));
        }
        state.authorized = true;
        Some(StartPermit(()))
    }

    /// Records stop without waiting for the authorized system call to finish.
    ///
    /// # Parameters
    pub(crate) fn request_stop(&self, reason: StopReason) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let won_before_start = !state.authorized;
        state.stopped = true;
        state.reason.get_or_insert(reason);
        self.cancellation.cancel();
        if let Some(cancellation) = &state.external_cancellation {
            cancellation.cancel();
        }
        won_before_start
    }

    #[cfg(test)]
    pub(crate) fn stop_requested(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }

    /// Performs the stop reason step in the managed command execution
    /// lifecycle.
    pub(crate) fn stop_reason(&self) -> Option<StopReason> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).reason
    }
}

impl StartGate {
    /// Performs the cancellation step in the managed command execution
    /// lifecycle.
    pub(crate) fn cancellation(&self) -> CommandCancellation {
        self.cancellation.clone()
    }

    /// Stores the attach external cancellation value used by this execution
    /// component.
    ///
    /// # Parameters
    /// * `cancellation`: Optional external cancellation source attached to this
    ///   start gate.
    pub(crate) fn attach_external_cancellation(&self, cancellation: Option<CommandCancellation>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            if let Some(cancellation) = cancellation {
                cancellation.cancel();
            }
        } else {
            state.external_cancellation = cancellation;
            if state
                .external_cancellation
                .as_ref()
                .is_some_and(CommandCancellation::is_cancelled)
            {
                state.stopped = true;
                state.reason = Some(StopReason::UserCancelled);
                self.cancellation.cancel();
            }
        }
    }
}

impl StopPort for StartGate {
    /// Records a stop request unless start authorization has already won.
    ///
    /// # Parameters
    /// * `reason`: Stop reason to retain if this request wins the race.
    fn request_stop(&self, reason: StopReason) {
        let _ = Self::request_stop(self, reason);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::io::Cursor;
    use std::io::Read;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::task::Waker;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use qubit_operation::StopReason;

    use super::StartGate;
    use crate::Command;
    use crate::CommandErrorKind;
    use crate::CommandRunOptions;
    use crate::CommandRunner;
    use crate::session::contracts::SessionBridge;

    struct ManualContextClock {
        domain: qubit_operation::ClockDomainId,
        ticks: Arc<AtomicU64>,
    }

    impl qubit_operation::ClockSource for ManualContextClock {
        fn now(&self) -> qubit_operation::MonotonicTime {
            qubit_operation::MonotonicTime::new(self.domain, self.ticks.load(Ordering::SeqCst))
        }

        fn register_deadline(
            &self,
            _at: qubit_operation::MonotonicTime,
            _wake: Waker,
        ) -> Box<dyn qubit_operation::TimerRegistration> {
            struct NoopRegistration;
            impl qubit_operation::TimerRegistration for NoopRegistration {
                fn cancel(self: Box<Self>) {}
            }
            Box::new(NoopRegistration)
        }
    }

    struct RecordingBridge {
        stdin: Mutex<Cursor<Vec<u8>>>,
        stdout: Mutex<Vec<u8>>,
        stderr: Mutex<Vec<u8>>,
        closed: AtomicBool,
        broken_pipe: AtomicBool,
        output_limit: Option<u64>,
        observed_output: AtomicU64,
    }

    impl RecordingBridge {
        fn new(stdin: &[u8], output_limit: Option<u64>) -> Self {
            Self {
                stdin: Mutex::new(Cursor::new(stdin.to_vec())),
                stdout: Mutex::new(Vec::new()),
                stderr: Mutex::new(Vec::new()),
                closed: AtomicBool::new(false),
                broken_pipe: AtomicBool::new(false),
                output_limit,
                observed_output: AtomicU64::new(0),
            }
        }

        fn record(&self, output: &Mutex<Vec<u8>>, bytes: &[u8]) {
            output.lock().unwrap().extend_from_slice(bytes);
            self.observed_output.fetch_add(bytes.len() as u64, Ordering::SeqCst);
        }
    }

    impl SessionBridge for RecordingBridge {
        fn read_stdin(&self, buffer: &mut [u8]) -> io::Result<usize> {
            self.stdin.lock().unwrap().read(buffer)
        }

        fn write_stdout(&self, bytes: &[u8]) -> io::Result<()> {
            self.record(&self.stdout, bytes);
            Ok(())
        }

        fn write_stderr(&self, bytes: &[u8]) -> io::Result<()> {
            self.record(&self.stderr, bytes);
            Ok(())
        }

        fn close_stdin(&self) {
            self.closed.store(true, Ordering::SeqCst);
        }

        fn stdin_broken_pipe(&self) -> bool {
            self.broken_pipe.load(Ordering::SeqCst)
        }

        fn record_stdin_broken_pipe(&self) {
            self.broken_pipe.store(true, Ordering::SeqCst);
        }

        fn output_budget_exceeded(&self) -> bool {
            self.output_limit
                .is_some_and(|limit| self.observed_output.load(Ordering::SeqCst) > limit)
        }

        fn output_budget_limit(&self) -> Option<u64> {
            self.output_limit
        }

        fn output_bytes_observed(&self) -> u64 {
            self.observed_output.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn test_stop_wins_start_gate() {
        let gate = StartGate::default();
        assert!(gate.request_stop(StopReason::UserCancelled));
        assert!(gate.authorize().is_none());
    }

    #[test]
    fn test_side_effect_authorization_wins_stop_gate() {
        let gate = StartGate::default();
        assert!(gate.authorize().is_some());
        assert!(!gate.request_stop(StopReason::UserCancelled));
        assert!(gate.stop_requested());
    }

    #[test]
    fn test_independent_activations_have_independent_stop_gates() {
        let first = StartGate::default();
        let second = StartGate::default();
        first.request_stop(StopReason::UserCancelled);
        assert!(first.authorize().is_none());
        assert!(second.authorize().is_some());
    }

    #[test]
    fn test_external_cancel_before_authorization_prevents_start() {
        let gate = StartGate::default();
        let cancellation = crate::CommandCancellation::new();
        gate.attach_external_cancellation(Some(cancellation.clone()));
        cancellation.cancel();
        assert!(gate.authorize().is_none());
        assert_eq!(gate.stop_reason(), Some(StopReason::UserCancelled));
    }

    #[cfg(unix)]
    #[test]
    fn test_session_bridge_moves_stdin_and_both_output_streams() {
        let bridge = Arc::new(RecordingBridge::new(b"from-session\n", None));
        let gate = StartGate::default();
        let output = CommandRunner::without_timeout()
            .run_with_tree_policy(
                Command::new("sh")
                    .arg("-c")
                    .arg("read value; printf '%s' \"$value\"; printf 'stderr' >&2"),
                CommandRunOptions::new().cancellation(gate.cancellation()),
                crate::command_runner::RunTreePolicy {
                    managed_policy: Some(crate::ProcessTreePolicy::Managed),
                    start_gate: Some(&gate),
                    session_bridge: Some(Arc::clone(&bridge) as Arc<dyn SessionBridge>),
                    ..Default::default()
                },
            )
            .expect("session command should complete");

        assert_eq!(output.stdout(), b"from-session");
        assert_eq!(bridge.stdout.lock().unwrap().as_slice(), b"from-session");
        assert_eq!(bridge.stderr.lock().unwrap().as_slice(), b"stderr");
        assert!(bridge.closed.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn test_session_output_budget_stops_child_with_configured_and_observed_counts() {
        let bridge = Arc::new(RecordingBridge::new(b"", Some(2)));
        let gate = StartGate::default();
        let error = CommandRunner::without_timeout()
            .run_with_tree_policy(
                Command::new("sh").arg("-c").arg("printf abc; exec sleep 30"),
                CommandRunOptions::new().cancellation(gate.cancellation()),
                crate::command_runner::RunTreePolicy {
                    managed_policy: Some(crate::ProcessTreePolicy::Managed),
                    start_gate: Some(&gate),
                    session_bridge: Some(Arc::clone(&bridge) as Arc<dyn SessionBridge>),
                    ..Default::default()
                },
            )
            .expect_err("aggregate session limit should stop the child");

        assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::OutputLimitExceeded { limit: 2, observed: 3 }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn test_active_command_observes_inherited_operation_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let ready = directory.path().join("deadline-active");
        let ready_arg = ready.to_string_lossy().into_owned();
        let domain = qubit_operation::ClockDomainId::new_unique();
        let ticks = Arc::new(AtomicU64::new(0));
        let clock: Arc<dyn qubit_operation::ClockSource> = Arc::new(ManualContextClock {
            domain,
            ticks: Arc::clone(&ticks),
        });
        let context = qubit_operation::OperationContext::new(qubit_operation::ContextOptions {
            clock: Some(clock),
            deadline: Some(qubit_operation::Deadline::at(qubit_operation::MonotonicTime::new(
                domain, 1,
            ))),
            budget: None,
        })
        .unwrap();
        let gate = Arc::new(StartGate::default());
        let run_gate = Arc::clone(&gate);
        let runner_context = context.clone();
        let runner = thread::spawn(move || {
            CommandRunner::without_timeout().run_with_tree_policy(
                Command::new("sh")
                    .arg("-c")
                    .arg("touch \"$1\"; exec sleep 30")
                    .arg("managed-test")
                    .arg(&ready_arg),
                CommandRunOptions::new().cancellation(run_gate.cancellation()),
                crate::command_runner::RunTreePolicy {
                    managed_policy: Some(crate::ProcessTreePolicy::Managed),
                    start_gate: Some(&run_gate),
                    operation_context: Some(runner_context),
                    ..Default::default()
                },
            )
        });
        let started = Instant::now();
        while !ready.exists() && started.elapsed() < Duration::from_secs(5) {
            thread::yield_now();
        }
        assert!(ready.exists(), "child did not reach its active marker");
        ticks.store(1, Ordering::SeqCst);
        let error = runner.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), CommandErrorKind::Cancelled);
        assert_eq!(context.stop_reason(), Some(StopReason::Timeout));
    }

    #[cfg(unix)]
    #[test]
    fn test_stop_before_commit_does_not_spawn_or_truncate() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("spawned");
        let tee = directory.path().join("tee");
        let marker_arg = marker.to_string_lossy().into_owned();
        fs::write(&tee, b"keep").unwrap();
        let gate = StartGate::default();
        gate.request_stop(StopReason::UserCancelled);
        let result = CommandRunner::without_timeout().run_with_tree_policy(
            Command::new("sh")
                .arg("-c")
                .arg("touch \"$1\"")
                .arg("managed-test")
                .arg(&marker_arg),
            CommandRunOptions::new()
                .cancellation(gate.cancellation())
                .tee_stdout_to_file(&tee),
            crate::command_runner::RunTreePolicy {
                managed_policy: Some(crate::ProcessTreePolicy::Managed),
                start_gate: Some(&gate),
                ..Default::default()
            },
        );
        assert_eq!(result.unwrap_err().kind(), CommandErrorKind::CancelledBeforeStart);
        assert!(!marker.exists());
        assert_eq!(fs::read(&tee).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn test_stop_after_spawn_reaches_child_and_waits_for_runner_cleanup() {
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
                crate::command_runner::RunTreePolicy {
                    managed_policy: Some(crate::ProcessTreePolicy::Managed),
                    start_gate: Some(&run_gate),
                    ..Default::default()
                },
            )
        });
        let started = Instant::now();
        let ready_path = directory.path().join("ready");
        while !ready_path.exists() && started.elapsed() < Duration::from_secs(5) {
            thread::yield_now();
        }
        assert!(ready_path.exists(), "child did not reach its readiness marker");
        gate.request_stop(StopReason::UserCancelled);
        let result = runner.join().expect("runner thread should join");
        assert_eq!(result.unwrap_err().kind(), CommandErrorKind::Cancelled);
    }
}
