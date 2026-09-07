// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use qubit_clock::BlockingSleeper;
use qubit_clock::MonotonicInstant;
use qubit_clock::TimeError;
use qubit_clock::Timer;

use super::command_io::CommandIo;
use super::finished_command::FinishedCommand;
use super::managed_child_process::ManagedChildProcess;
use super::process_terminator::ProcessTerminator;
use super::run_event::RunEvent;
use super::stop_reason::StopReason;
use super::wait_policy::next_sleep;
use crate::CommandCancellation;
use crate::CommandCleanupFailure;
use crate::CommandError;

/// Maximum delay before a cancellation-aware wait observes cancellation.
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Running command state that owns process and I/O helper lifetimes.
#[must_use = "a running command must be waited on to collect its process and I/O"]
pub(in crate::command_runner) struct RunningCommand {
    /// Human-readable command text for diagnostics.
    command_text: String,
    /// Child process managed by the command runner.
    child_process: ManagedChildProcess,
    /// Output readers and optional stdin writer.
    io: CommandIo,
    /// Time when the child process started being monitored.
    started_at: MonotonicInstant,
    /// Timer sharing the same monotonic domain as the start instant.
    timer: Arc<dyn Timer>,
    /// Optional shared cancellation handle.
    cancellation_token: Option<CommandCancellation>,
}

impl RunningCommand {
    /// Creates a running command state object.
    ///
    /// # Parameters
    ///
    /// * `command_text` - Human-readable command text for diagnostics.
    /// * `child_process` - Child process managed by the runner.
    /// * `io` - Output readers and optional stdin writer.
    /// * `started_at` - Monotonic instant sampled immediately after spawning.
    /// * `timer` - Timer in the same clock domain as `started_at`.
    /// * `cancellation_token` - Optional shared cancellation handle.
    ///
    /// # Returns
    ///
    /// Running command state that owns the process and its I/O helpers.
    #[inline]
    pub(in crate::command_runner) fn new(
        command_text: String,
        child_process: ManagedChildProcess,
        io: CommandIo,
        started_at: MonotonicInstant,
        timer: Arc<dyn Timer>,
        cancellation_token: Option<CommandCancellation>,
    ) -> Self {
        Self {
            command_text,
            child_process,
            io,
            started_at,
            timer,
            cancellation_token,
        }
    }

    /// Waits for the child process to complete, time out, or be cancelled.
    ///
    /// This method blocks the current thread. Without a timeout or a
    /// cancellation handle it delegates directly to the child process's
    /// blocking wait operation.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Optional command timeout.
    ///
    /// # Returns
    ///
    /// Finished command output when the child exits normally.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if waiting, timeout handling, cancellation,
    /// output collection, or stdin writing fails.
    pub(in crate::command_runner) fn wait_for_completion(
        mut self,
        timeout: Option<Duration>,
    ) -> Result<FinishedCommand, CommandError> {
        let event = self.wait_for_event(timeout);
        self.resolve_event(event, timeout)
    }

    /// Waits until process monitoring produces one terminal event.
    ///
    /// This phase only observes the child, cancellation token, and timer. It
    /// does not terminate the process, finalize helpers, or construct a
    /// [`CommandError`].
    fn wait_for_event(&mut self, timeout: Option<Duration>) -> RunEvent {
        if timeout.is_none() && self.cancellation_token.is_none() {
            return match self.child_process.wait() {
                Ok(status) => RunEvent::Exited(status),
                Err(source) => RunEvent::WaitFailed(source),
            };
        }

        let mut timeout_poll_count = 0;
        loop {
            let maybe_status = match self.child_process.try_wait() {
                Ok(status) => status,
                Err(source) => return RunEvent::WaitFailed(source),
            };
            if let Some(status) = maybe_status {
                return RunEvent::Exited(status);
            }
            if self
                .cancellation_token
                .as_ref()
                .is_some_and(CommandCancellation::is_cancelled)
            {
                return RunEvent::Cancelled { status: None };
            }
            let sleep = match timeout {
                Some(timeout) => {
                    let elapsed = match self.elapsed() {
                        Ok(elapsed) => elapsed,
                        Err(source) => {
                            return RunEvent::TimeFailed {
                                source,
                                status: None,
                            };
                        }
                    };
                    if elapsed >= timeout {
                        return RunEvent::TimedOut {
                            timeout,
                            status: None,
                        };
                    }
                    let sleep = next_sleep(timeout, elapsed, timeout_poll_count);
                    timeout_poll_count = timeout_poll_count.saturating_add(1);
                    sleep
                }
                None => CANCELLATION_POLL_INTERVAL,
            };
            if let Err(source) = BlockingSleeper::new(Arc::clone(&self.timer)).sleep_for(sleep) {
                return RunEvent::TimeFailed {
                    source,
                    status: None,
                };
            }
        }
    }

    /// Waits for I/O helpers after the direct child has exited.
    ///
    /// The original timeout continues to be measured from `started_at`, so
    /// inherited pipes cannot restart the command deadline after child exit.
    fn wait_for_io_event(&mut self, status: ExitStatus, timeout: Option<Duration>) -> RunEvent {
        if timeout.is_some() || self.cancellation_token.is_some() {
            let mut timeout_poll_count = 0;
            while !self.io.is_finished() {
                if self
                    .cancellation_token
                    .as_ref()
                    .is_some_and(CommandCancellation::is_cancelled)
                {
                    return RunEvent::Cancelled {
                        status: Some(status),
                    };
                }
                let sleep = match timeout {
                    Some(timeout) => {
                        let elapsed = match self.elapsed() {
                            Ok(elapsed) => elapsed,
                            Err(source) => {
                                return RunEvent::TimeFailed {
                                    source,
                                    status: Some(status),
                                };
                            }
                        };
                        if elapsed >= timeout {
                            return RunEvent::TimedOut {
                                timeout,
                                status: Some(status),
                            };
                        }
                        let sleep = next_sleep(timeout, elapsed, timeout_poll_count);
                        timeout_poll_count = timeout_poll_count.saturating_add(1);
                        sleep
                    }
                    None => CANCELLATION_POLL_INTERVAL,
                };
                if let Err(source) = BlockingSleeper::new(Arc::clone(&self.timer)).sleep_for(sleep)
                {
                    return RunEvent::TimeFailed {
                        source,
                        status: Some(status),
                    };
                }
            }
        }
        RunEvent::Exited(status)
    }

    /// Resolves one monitoring event through the single finalization pipeline.
    fn resolve_event(
        mut self,
        event: RunEvent,
        timeout: Option<Duration>,
    ) -> Result<FinishedCommand, CommandError> {
        let event = match event.into_exit_status() {
            Ok(status) => self.wait_for_io_event(status, timeout),
            Err(reason) => return self.stop_and_finalize(reason),
        };
        match event.into_exit_status() {
            Ok(status) => self.complete(status),
            Err(reason) => self.stop_and_finalize(reason),
        }
    }

    /// Stops the managed process tree and finalizes all I/O helpers.
    ///
    /// # Parameters
    ///
    /// * `reason` - Primary reason that monitoring stopped the command.
    ///
    /// # Returns
    ///
    /// This method always returns an error after completing cleanup.
    fn stop_and_finalize(mut self, reason: StopReason) -> Result<FinishedCommand, CommandError> {
        let observed_status = reason.observed_status();
        let retains_output = reason.retains_termination_output();
        let outcome =
            match ProcessTerminator::new(&mut self.child_process).terminate(observed_status) {
                Ok(outcome) => outcome,
                Err(failure) => {
                    let error = failure.into_command_error(reason, &self.command_text);
                    return Err(self.finish_without_status(error));
                }
            };
        let status = observed_status.unwrap_or(outcome.status);
        if !retains_output {
            let error = reason
                .into_primary_error(self.command_text.clone(), None)
                .with_cleanup_failures(outcome.cleanup_failures);
            return Err(self.finish_without_status(error));
        }
        let command_text = self.command_text.clone();
        let (finished, io_cleanup_failures) = self.complete_after_termination(status);
        let finished = match finished {
            Ok(finished) => finished,
            Err(error) => {
                if matches!(
                    reason,
                    StopReason::TimeFailed {
                        status: Some(_),
                        ..
                    }
                ) {
                    return Err(reason
                        .into_error_after_finalize(command_text, error)
                        .with_cleanup_failures(outcome.cleanup_failures)
                        .with_cleanup_failures(io_cleanup_failures));
                }
                return Err(error
                    .with_cleanup_failures(outcome.cleanup_failures)
                    .with_cleanup_failures(io_cleanup_failures));
            }
        };
        Err(reason
            .into_primary_error(finished.command_text, Some(Box::new(finished.output)))
            .with_cleanup_failures(outcome.cleanup_failures)
            .with_cleanup_failures(io_cleanup_failures))
    }

    /// Completes a known-exited command by joining all I/O helpers.
    ///
    /// # Parameters
    ///
    /// * `status` - Exit status reported by the child process.
    ///
    /// # Returns
    ///
    /// Finished command output with retained stdout and stderr bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if output collection or stdin writing fails.
    fn complete(self, status: ExitStatus) -> Result<FinishedCommand, CommandError> {
        let Self {
            command_text,
            io,
            started_at,
            timer,
            ..
        } = self;
        let output = io.collect(&command_text, status, move || {
            timer.clock().now().duration_since(started_at)
        })?;
        Ok(FinishedCommand {
            command_text,
            output,
        })
    }

    /// Completes a terminated command after cancelling and joining I/O helpers.
    ///
    /// # Parameters
    ///
    /// * `status` - Exit status reported by the child process.
    ///
    /// # Returns
    ///
    /// Finished command output after all helpers have been cancelled and
    /// joined.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if a completed helper or elapsed-time sampling
    /// fails.
    fn complete_after_termination(
        self,
        status: ExitStatus,
    ) -> (
        Result<FinishedCommand, CommandError>,
        Vec<CommandCleanupFailure>,
    ) {
        let Self {
            command_text,
            io,
            started_at,
            timer,
            ..
        } = self;
        let (output, cleanup_failures) = io.cancel_and_collect(&command_text, status, move || {
            timer.clock().now().duration_since(started_at)
        });
        (
            output.map(|output| FinishedCommand {
                command_text,
                output,
            }),
            cleanup_failures,
        )
    }

    /// Returns elapsed time in the injected timer's clock domain.
    ///
    /// # Returns
    ///
    /// Duration since the child process was spawned.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError`] if the timer violates the retained clock domain or
    /// monotonic ordering.
    fn elapsed(&self) -> Result<Duration, TimeError> {
        self.timer.clock().now().duration_since(self.started_at)
    }

    /// Completes without process output and preserves the primary error.
    ///
    /// This method always invokes helper cancellation and joining before
    /// returning `primary`, retaining every cleanup failure.
    #[must_use]
    fn finish_without_status(self, primary: CommandError) -> CommandError {
        let cleanup_failures = self.io.cancel_and_join(&self.command_text);
        primary.with_cleanup_failures(cleanup_failures)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    #[cfg(unix)]
    use std::io::Read;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::Command as ProcessCommand;
    use std::process::ExitStatus;
    use std::process::Stdio;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use process_wrap::std::ChildWrapper;
    use qubit_clock::ManualMonotonicClock;
    use qubit_clock::MonotonicClock;
    use qubit_clock::TimeError;

    use super::super::captured_output::CapturedOutput;
    use super::super::command_io::CommandIo;
    use super::super::finished_command::FinishedCommand;
    use super::super::io_cancellation::IoCancellation;
    use super::super::io_cancellation_token::IoCancellationToken;
    use super::super::managed_child_process::ManagedChildProcess;
    use super::super::output_capture_error::OutputCaptureError;
    use super::super::output_reader::OutputReader;
    use super::super::run_event::RunEvent;
    use super::super::stdin_writer::StdinWriter;
    use super::RunningCommand;
    use crate::CommandCleanupFailure;
    use crate::CommandError;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;
    use crate::OutputStream;

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code as u32)
    }

    #[derive(Debug)]
    struct ScriptedChild {
        inner: Box<dyn ChildWrapper>,
        kill_error: Option<io::Error>,
        wait_result: Option<io::Result<ExitStatus>>,
        try_wait_results: VecDeque<io::Result<Option<ExitStatus>>>,
    }

    impl ScriptedChild {
        fn new(inner: Box<dyn ChildWrapper>) -> Self {
            Self {
                inner,
                kill_error: None,
                wait_result: None,
                try_wait_results: VecDeque::new(),
            }
        }

        fn kill_error(mut self, source: io::Error) -> Self {
            self.kill_error = Some(source);
            self
        }

        fn wait_status(mut self, exit_status: ExitStatus) -> Self {
            self.wait_result = Some(Ok(exit_status));
            self
        }

        fn try_wait_results(
            mut self,
            results: impl IntoIterator<Item = io::Result<Option<ExitStatus>>>,
        ) -> Self {
            self.try_wait_results.extend(results);
            self
        }
    }

    impl ChildWrapper for ScriptedChild {
        fn inner(&self) -> &dyn ChildWrapper {
            self.inner.as_ref()
        }

        fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
            self.inner.as_mut()
        }

        fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
            self.inner
        }

        fn start_kill(&mut self) -> io::Result<()> {
            match self.kill_error.take() {
                Some(source) => Err(source),
                None => Ok(()),
            }
        }

        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            self.try_wait_results
                .pop_front()
                .unwrap_or_else(|| self.inner.try_wait())
        }

        fn wait(&mut self) -> io::Result<ExitStatus> {
            self.wait_result.take().unwrap_or_else(|| self.inner.wait())
        }
    }

    fn raw_child() -> Box<dyn ChildWrapper> {
        let mut child = ProcessCommand::new("rustc")
            .arg("--version")
            .stdout(Stdio::null())
            .spawn()
            .expect("test child should spawn");
        child.wait().expect("test child should be reaped");
        Box::new(child)
    }

    fn terminating_child(exit_status: ExitStatus) -> ManagedChildProcess {
        ManagedChildProcess::new(
            Box::new(ScriptedChild::new(raw_child()).wait_status(exit_status)),
            true,
        )
    }

    fn fallback_race_child(exit_status: ExitStatus) -> ManagedChildProcess {
        let direct = ScriptedChild::new(raw_child())
            .kill_error(io::Error::other("child termination failed"));
        let tree = ScriptedChild::new(Box::new(direct))
            .kill_error(io::Error::other("tree termination failed"))
            .try_wait_results(
                (0..8)
                    .map(|_| Ok(None))
                    .chain(std::iter::once(Ok(Some(exit_status)))),
            );
        ManagedChildProcess::new(Box::new(tree), true)
    }

    fn completed_reader(result: Result<CapturedOutput, OutputCaptureError>) -> OutputReader {
        let (cancellation, token) =
            IoCancellation::pair().expect("reader cancellation should create");
        OutputReader::new(
            thread::spawn(move || {
                wait_for_cancellation(&token);
                result
            }),
            cancellation,
        )
    }

    fn completed_writer(result: io::Result<()>) -> StdinWriter {
        let (cancellation, token) =
            IoCancellation::pair().expect("writer cancellation should create");
        StdinWriter::new(
            thread::spawn(move || {
                wait_for_cancellation(&token);
                result
            }),
            cancellation,
        )
    }

    #[cfg(unix)]
    fn wait_for_cancellation(token: &IoCancellationToken) {
        let mut byte = [0_u8; 1];
        loop {
            match (&token.wakeup).read(&mut byte) {
                Ok(0) => thread::yield_now(),
                Ok(_) => return,
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => thread::yield_now(),
                Err(source) => panic!("cancellation wakeup should be readable: {source}"),
            }
        }
    }

    #[cfg(windows)]
    fn wait_for_cancellation(token: &IoCancellationToken) {
        while !token.is_cancelled() {
            thread::yield_now();
        }
    }

    fn successful_reader() -> OutputReader {
        completed_reader(Ok(CapturedOutput::default()))
    }

    fn failing_stdout() -> OutputReader {
        completed_reader(Err(OutputCaptureError::Read {
            source: io::Error::other("stdout read failed"),
            output: CapturedOutput::default(),
        }))
    }

    fn failing_stderr() -> OutputReader {
        completed_reader(Err(OutputCaptureError::Write {
            path: PathBuf::from("stderr.log"),
            source: io::Error::other("stderr write failed"),
            output: CapturedOutput::default(),
        }))
    }

    fn io_with_failures() -> CommandIo {
        CommandIo::new(
            failing_stdout(),
            failing_stderr(),
            Some(completed_writer(Err(io::Error::other(
                "stdin write failed",
            )))),
        )
    }

    fn completed_io_with_cancellation_failures() -> CommandIo {
        let stdout = OutputReader::new(
            thread::spawn(|| {
                Err(OutputCaptureError::Read {
                    source: io::Error::other("stdout read failed"),
                    output: CapturedOutput::default(),
                })
            }),
            IoCancellation::failing("stdout cancellation failed"),
        );
        let stderr = OutputReader::new(
            thread::spawn(|| {
                Err(OutputCaptureError::Write {
                    path: PathBuf::from("stderr.log"),
                    source: io::Error::other("stderr write failed"),
                    output: CapturedOutput::default(),
                })
            }),
            IoCancellation::failing("stderr cancellation failed"),
        );
        let stdin = StdinWriter::new(
            thread::spawn(|| Err(io::Error::other("stdin write failed"))),
            IoCancellation::failing("stdin cancellation failed"),
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        while !(stdout.is_finished() && stderr.is_finished() && stdin.is_finished()) {
            assert!(
                Instant::now() < deadline,
                "failing helpers should finish before finalization"
            );
            thread::yield_now();
        }
        CommandIo::new(stdout, stderr, Some(stdin))
    }

    fn running(child: ManagedChildProcess, io: CommandIo) -> RunningCommand {
        let clock = ManualMonotonicClock::new_shared();
        let started_at = clock.now();
        RunningCommand::new(
            "test command".to_owned(),
            child,
            io,
            started_at,
            clock.new_timer(),
            None,
        )
    }

    fn running_with_elapsed_failure(child: ManagedChildProcess, io: CommandIo) -> RunningCommand {
        let started_clock = ManualMonotonicClock::new_shared();
        let timer_clock = ManualMonotonicClock::new_shared();
        RunningCommand::new(
            "test command".to_owned(),
            child,
            io,
            started_clock.now(),
            timer_clock.new_timer(),
            None,
        )
    }

    fn expect_error(result: Result<FinishedCommand, CommandError>, message: &str) -> CommandError {
        match result {
            Ok(_) => panic!("{message}"),
            Err(error) => error,
        }
    }

    fn timeout_with_output_failure() -> CommandError {
        expect_error(
            running(
                terminating_child(status(0)),
                CommandIo::new(failing_stdout(), successful_reader(), None),
            )
            .resolve_event(
                RunEvent::TimedOut {
                    timeout: Duration::from_secs(2),
                    status: Some(status(0)),
                },
                Some(Duration::from_secs(2)),
            ),
            "output failure should supersede timeout after termination",
        )
    }

    fn cancellation_with_fallback_and_helper_failures() -> CommandError {
        expect_error(
            running_with_elapsed_failure(
                fallback_race_child(status(0)),
                completed_io_with_cancellation_failures(),
            )
            .resolve_event(RunEvent::Cancelled { status: None }, None),
            "termination and helper cleanup failures should retain canonical order",
        )
    }

    fn time_failure_with_helper_failures() -> CommandError {
        expect_error(
            running(terminating_child(status(0)), io_with_failures()).resolve_event(
                RunEvent::TimeFailed {
                    source: TimeError::InstantOverflow,
                    status: Some(status(0)),
                },
                None,
            ),
            "time failure should remain primary",
        )
    }

    fn wait_failure_with_helper_failures() -> CommandError {
        expect_error(
            running(terminating_child(status(0)), io_with_failures())
                .resolve_event(RunEvent::WaitFailed(io::Error::other("wait failed")), None),
            "wait failure should remain primary",
        )
    }

    fn cleanup_order(error: &CommandError) -> Vec<&'static str> {
        error
            .cleanup_failures()
            .iter()
            .map(|failure| match failure {
                CommandCleanupFailure::StdoutRead { .. } => "stdout-read",
                CommandCleanupFailure::StdoutCancellation { .. } => "stdout-cancel",
                CommandCleanupFailure::StderrWrite { .. } => "stderr-write",
                CommandCleanupFailure::StderrCancellation { .. } => "stderr-cancel",
                CommandCleanupFailure::Stdin { .. } => "stdin-write",
                CommandCleanupFailure::StdinCancellation { .. } => "stdin-cancel",
                CommandCleanupFailure::ProcessTreeTermination { .. } => "process-tree",
                CommandCleanupFailure::ChildTermination { .. } => "direct-child",
                other => panic!("unexpected cleanup failure: {other:?}"),
            })
            .collect()
    }

    #[derive(Clone, Copy)]
    enum ExpectedReason {
        ReadOutput,
        TimeFailed,
        WaitFailed,
    }

    #[test]
    fn test_resolve_event_preserves_error_priority_and_cleanup_order() {
        let cases = [
            (
                "timeout plus output read failure",
                timeout_with_output_failure as fn() -> CommandError,
                CommandErrorKind::ReadOutputFailed,
                ExpectedReason::ReadOutput,
                true,
                &[][..],
            ),
            (
                "cancellation plus tree fallback and helper failures",
                cancellation_with_fallback_and_helper_failures,
                CommandErrorKind::TimeFailed,
                ExpectedReason::TimeFailed,
                false,
                &[
                    "process-tree",
                    "direct-child",
                    "stdout-read",
                    "stdout-cancel",
                    "stderr-write",
                    "stderr-cancel",
                    "stdin-write",
                    "stdin-cancel",
                ][..],
            ),
            (
                "time failure plus helper failures",
                time_failure_with_helper_failures,
                CommandErrorKind::TimeFailed,
                ExpectedReason::TimeFailed,
                false,
                &["stdout-read", "stderr-write", "stdin-write"][..],
            ),
            (
                "wait failure plus helper failures",
                wait_failure_with_helper_failures,
                CommandErrorKind::WaitFailed,
                ExpectedReason::WaitFailed,
                false,
                &["stdout-read", "stderr-write", "stdin-write"][..],
            ),
        ];

        for (name, run, expected_kind, expected_reason, has_output, expected_cleanup) in cases {
            let error = run();
            assert_eq!(error.kind(), expected_kind, "{name}");
            assert!(
                matches!(
                    (error.reason(), expected_reason),
                    (
                        CommandErrorReason::ReadOutputFailed {
                            stream: OutputStream::Stdout,
                            ..
                        },
                        ExpectedReason::ReadOutput
                    ) | (
                        CommandErrorReason::TimeFailed { .. },
                        ExpectedReason::TimeFailed
                    ) | (
                        CommandErrorReason::WaitFailed { .. },
                        ExpectedReason::WaitFailed
                    )
                ),
                "{name}"
            );
            assert_eq!(error.output().is_some(), has_output, "{name}");
            assert_eq!(cleanup_order(&error), expected_cleanup, "{name}");
        }
    }
}
