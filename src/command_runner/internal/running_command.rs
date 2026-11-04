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
use super::total_output_budget::TotalOutputBudget;
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
    /// Optional byte budget shared by this command's stdout and stderr.
    total_budget: Option<Arc<TotalOutputBudget>>,
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
    /// * `total_budget` - Optional byte budget shared across this run's stdout
    ///   and stderr capture.
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
        total_budget: Option<Arc<TotalOutputBudget>>,
    ) -> Self {
        Self {
            command_text,
            child_process,
            io,
            started_at,
            timer,
            cancellation_token,
            total_budget,
        }
    }

    /// Waits for the child process to complete, time out, be cancelled, or
    /// exceed its total output budget.
    ///
    /// This method blocks the current thread. It delegates directly to the
    /// child process's blocking wait operation only when no timeout,
    /// cancellation handle, or total output budget is configured. Otherwise,
    /// it monitors for process exit, timeout, cancellation, and budget
    /// exhaustion.
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
    /// This phase monitors the child, cancellation token, timer, and total
    /// output budget. It does not terminate the process, finalize helpers, or
    /// construct a [`CommandError`].
    fn wait_for_event(&mut self, timeout: Option<Duration>) -> RunEvent {
        if timeout.is_none() && self.cancellation_token.is_none() && self.total_budget.is_none() {
            return match self.child_process.wait() {
                Ok(status) => RunEvent::Exited(status),
                Err(source) => RunEvent::WaitFailed(source),
            };
        }

        let mut timeout_poll_count = 0;
        loop {
            if let Some((limit, observed)) = self.total_budget.as_ref().and_then(|budget| budget.exceeded()) {
                let status = self.child_process.try_wait().ok().flatten();
                return RunEvent::OutputLimitExceeded {
                    limit,
                    observed,
                    status,
                };
            }
            let maybe_status = match self.child_process.try_wait() {
                Ok(status) => status,
                Err(source) => return RunEvent::WaitFailed(source),
            };
            if let Some(status) = maybe_status {
                return RunEvent::Exited(status);
            }
            if let Err(event) = self.check_cancellation_and_wait(None, timeout, &mut timeout_poll_count) {
                return event;
            }
        }
    }

    /// Waits for I/O helpers after the direct child has exited.
    ///
    /// The original timeout continues to be measured from `started_at`, so
    /// inherited pipes cannot restart the command deadline after child exit.
    fn wait_for_io_event(&mut self, status: ExitStatus, timeout: Option<Duration>) -> RunEvent {
        if timeout.is_some() || self.cancellation_token.is_some() || self.total_budget.is_some() {
            let mut timeout_poll_count = 0;
            while !self.io.is_finished() {
                if let Some((limit, observed)) = self.total_budget.as_ref().and_then(|budget| budget.exceeded()) {
                    return RunEvent::OutputLimitExceeded {
                        limit,
                        observed,
                        status: Some(status),
                    };
                }
                if let Err(event) = self.check_cancellation_and_wait(Some(status), timeout, &mut timeout_poll_count) {
                    return event;
                }
            }
        }
        if let Some((limit, observed)) = self.total_budget.as_ref().and_then(|budget| budget.exceeded()) {
            return RunEvent::OutputLimitExceeded {
                limit,
                observed,
                status: Some(status),
            };
        }
        RunEvent::Exited(status)
    }

    /// Checks cancellation and the original deadline, then waits for the next
    /// poll.
    ///
    /// Returns `Ok(())` when monitoring should continue. Returns `Err(event)`
    /// when cancellation, a time error, or the timeout ends the current phase.
    /// The optional exit status is retained in any terminal event produced
    /// here.
    fn check_cancellation_and_wait(
        &self,
        status: Option<ExitStatus>,
        timeout: Option<Duration>,
        timeout_poll_count: &mut usize,
    ) -> Result<(), RunEvent> {
        if self
            .cancellation_token
            .as_ref()
            .is_some_and(CommandCancellation::is_cancelled)
        {
            return Err(RunEvent::Cancelled { status });
        }
        let sleep = match timeout {
            Some(timeout) => {
                let elapsed = self
                    .elapsed()
                    .map_err(|source| RunEvent::TimeFailed { source, status })?;
                if elapsed >= timeout {
                    return Err(RunEvent::TimedOut { timeout, status });
                }
                let sleep = next_sleep(timeout, elapsed, *timeout_poll_count);
                *timeout_poll_count = timeout_poll_count.saturating_add(1);
                sleep
            }
            None => CANCELLATION_POLL_INTERVAL,
        };
        BlockingSleeper::new(Arc::clone(&self.timer))
            .sleep_for(sleep)
            .map_err(|source| RunEvent::TimeFailed { source, status })
    }

    /// Resolves one monitoring event through the single finalization pipeline.
    fn resolve_event(mut self, event: RunEvent, timeout: Option<Duration>) -> Result<FinishedCommand, CommandError> {
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
        let output_limit = match &reason {
            StopReason::OutputLimitExceeded { limit, observed, .. } => Some((*limit, *observed)),
            _ => None,
        };
        let total_budget = self.total_budget.clone();
        let outcome = match ProcessTerminator::new(&mut self.child_process).terminate(observed_status) {
            Ok(outcome) => outcome,
            Err(failure) => {
                let mut error = failure.into_command_error(reason, &self.command_text);
                if output_limit.is_some()
                    && let Some(budget) = total_budget.as_ref()
                {
                    error = error.with_captured_output(budget.captured_output());
                }
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
                if let StopReason::OutputLimitExceeded { limit, observed, .. } = reason {
                    let captured = total_budget.as_ref().map_or_else(
                        || error.captured_output().cloned(),
                        |budget| Some(budget.captured_output_with_joined(error.captured_output())),
                    );
                    let mut primary = CommandError::from_reason(
                        command_text,
                        crate::CommandErrorReason::OutputLimitExceeded { limit, observed },
                        None,
                    );
                    if let Some(captured) = captured {
                        primary = primary.with_captured_output(captured);
                    }
                    return Err(primary
                        .with_cleanup_failures(outcome.cleanup_failures)
                        .with_cleanup_failures(io_cleanup_failures)
                        .with_cleanup_failures(error.into_cleanup_failures()));
                }
                return Err(reason
                    .into_error_after_finalize(command_text, error)
                    .with_cleanup_failures(outcome.cleanup_failures)
                    .with_cleanup_failures(io_cleanup_failures));
            }
        };
        if let StopReason::OutputLimitExceeded { limit, observed, .. } = reason {
            let captured = finished.output.into_captured_output();
            return Err(CommandError::from_reason(
                finished.command_text,
                crate::CommandErrorReason::OutputLimitExceeded { limit, observed },
                None,
            )
            .with_captured_output(captured)
            .with_cleanup_failures(outcome.cleanup_failures)
            .with_cleanup_failures(io_cleanup_failures));
        }
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
        Ok(FinishedCommand { command_text, output })
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
    ) -> (Result<FinishedCommand, CommandError>, Vec<CommandCleanupFailure>) {
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
            output.map(|output| FinishedCommand { command_text, output }),
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
    fn finish_without_status(self, primary: CommandError) -> CommandError {
        let cleanup_failures = self.io.cancel_and_join(&self.command_text);
        primary.with_cleanup_failures(cleanup_failures)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    use std::io::Cursor;
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
    use std::sync::Arc;
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::sync::Weak;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use process_wrap::std::ChildWrapper;
    use qubit_clock::ClockDomain;
    use qubit_clock::ManualMonotonicClock;
    use qubit_clock::MonotonicClock;
    use qubit_clock::MonotonicInstant;
    use qubit_clock::TimeError;
    use qubit_clock::Timer;
    use qubit_clock::TimerFuture;

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
    use super::super::total_output_budget::TotalOutputBudget;
    use super::RunningCommand;
    use crate::CommandCleanupFailure;
    use crate::CommandError;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;

    struct FailingTimer {
        clock: CountingClock,
        at_calls: Arc<AtomicUsize>,
    }

    #[derive(Clone)]
    struct CountingClock {
        inner: Arc<ManualMonotonicClock>,
        now_calls: Arc<AtomicUsize>,
    }

    impl MonotonicClock for CountingClock {
        fn domain(&self) -> ClockDomain {
            self.inner.domain()
        }

        fn now(&self) -> MonotonicInstant {
            self.now_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.now()
        }

        fn new_timer(&self) -> Arc<dyn Timer> {
            self.inner.new_timer()
        }
    }

    struct ReadyCountingTimer {
        clock: CountingClock,
        at_calls: Arc<AtomicUsize>,
    }

    impl Timer for ReadyCountingTimer {
        fn clock(&self) -> &dyn MonotonicClock {
            &self.clock
        }

        fn at(&self, _deadline: MonotonicInstant) -> Result<TimerFuture, TimeError> {
            self.at_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(async { Ok(()) }))
        }
    }

    impl Timer for FailingTimer {
        fn clock(&self) -> &dyn MonotonicClock {
            &self.clock
        }

        fn at(&self, _deadline: MonotonicInstant) -> Result<TimerFuture, TimeError> {
            self.at_calls.fetch_add(1, Ordering::SeqCst);
            Err(TimeError::InstantOverflow)
        }
    }

    fn failing_timer() -> (Arc<FailingTimer>, Arc<AtomicUsize>) {
        let at_calls = Arc::new(AtomicUsize::new(0));
        let now_calls = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(FailingTimer {
                clock: CountingClock {
                    inner: ManualMonotonicClock::new_shared(),
                    now_calls,
                },
                at_calls: Arc::clone(&at_calls),
            }),
            at_calls,
        )
    }

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

        fn try_wait_results(mut self, results: impl IntoIterator<Item = io::Result<Option<ExitStatus>>>) -> Self {
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
        ManagedChildProcess::new(Box::new(ScriptedChild::new(raw_child()).wait_status(exit_status)), true)
    }

    fn fallback_race_child(exit_status: ExitStatus) -> ManagedChildProcess {
        let direct = ScriptedChild::new(raw_child()).kill_error(io::Error::other("child termination failed"));
        let tree = ScriptedChild::new(Box::new(direct))
            .kill_error(io::Error::other("tree termination failed"))
            .try_wait_results((0..8).map(|_| Ok(None)).chain(std::iter::once(Ok(Some(exit_status)))));
        ManagedChildProcess::new(Box::new(tree), true)
    }

    fn completed_reader(result: Result<CapturedOutput, OutputCaptureError>) -> OutputReader {
        let (cancellation, token) = IoCancellation::pair().expect("reader cancellation should create");
        OutputReader::new(
            thread::spawn(move || {
                wait_for_cancellation(&token);
                result
            }),
            cancellation,
        )
    }

    fn completed_writer(result: io::Result<()>) -> StdinWriter {
        let (cancellation, token) = IoCancellation::pair().expect("writer cancellation should create");
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

    fn immediate_successful_reader() -> OutputReader {
        let (cancellation, _token) = IoCancellation::pair().expect("reader cancellation should create");
        OutputReader::new(thread::spawn(|| Ok(CapturedOutput::default())), cancellation)
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
            Some(completed_writer(Err(io::Error::other("stdin write failed")))),
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
            None,
        )
    }

    fn running_with_budget(
        child: ManagedChildProcess,
        io: CommandIo,
        budget: Arc<TotalOutputBudget>,
    ) -> RunningCommand {
        let clock = ManualMonotonicClock::new_shared();
        let started_at = clock.now();
        RunningCommand::new(
            "test command".to_owned(),
            child,
            io,
            started_at,
            clock.new_timer(),
            None,
            Some(budget),
        )
    }

    fn running_with_budget_and_timer(
        child: ManagedChildProcess,
        io: CommandIo,
        budget: Arc<TotalOutputBudget>,
        timer: Arc<dyn Timer>,
    ) -> RunningCommand {
        let started_at = timer.clock().now();
        RunningCommand::new(
            "test command".to_owned(),
            child,
            io,
            started_at,
            timer,
            None,
            Some(budget),
        )
    }

    #[derive(Debug)]
    struct AuditChildState {
        status: Mutex<Option<ExitStatus>>,
        changed: Condvar,
        events: Mutex<Vec<&'static str>>,
        wait_started: Mutex<Option<mpsc::Sender<()>>>,
    }

    #[derive(Debug)]
    struct AuditChild {
        state: Arc<AuditChildState>,
    }

    impl AuditChild {
        fn new(events: Arc<AuditChildState>) -> Self {
            Self { state: events }
        }

        fn release(&self, status: ExitStatus) {
            *self.state.status.lock().expect("audit child state should lock") = Some(status);
            self.state.changed.notify_all();
        }
    }

    impl ChildWrapper for AuditChild {
        fn inner(&self) -> &dyn ChildWrapper {
            self
        }
        fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
            self
        }
        fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
            self
        }

        fn start_kill(&mut self) -> io::Result<()> {
            self.state.events.lock().expect("events should lock").push("kill");
            self.release(status(137));
            Ok(())
        }

        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            Ok(*self.state.status.lock().expect("audit child state should lock"))
        }

        fn wait(&mut self) -> io::Result<ExitStatus> {
            self.state.events.lock().expect("events should lock").push("wait");
            if let Some(wait_started) = self
                .state
                .wait_started
                .lock()
                .expect("wait-started signal should lock")
                .as_ref()
            {
                let _ = wait_started.send(());
            }
            let mut status = self.state.status.lock().expect("audit child state should lock");
            while status.is_none() {
                status = self.state.changed.wait(status).expect("audit child wait should wake");
            }
            Ok(status.expect("released child has an exit status"))
        }
    }

    struct AuditTimer {
        clock: CountingClock,
        registered: mpsc::Sender<()>,
    }

    impl Timer for AuditTimer {
        fn clock(&self) -> &dyn MonotonicClock {
            &self.clock
        }

        fn at(&self, deadline: MonotonicInstant) -> Result<TimerFuture, TimeError> {
            let _ = self.registered.send(());
            self.clock.inner.new_timer().at(deadline)
        }
    }

    fn audit_read_failure_reader(sender: mpsc::Sender<Instant>, events: Arc<AuditChildState>) -> OutputReader {
        let (cancellation, _token) = IoCancellation::pair().expect("reader cancellation should create");
        OutputReader::new(
            thread::spawn(move || {
                events.events.lock().expect("events should lock").push("reader-failed");
                sender.send(Instant::now()).expect("test should receive reader failure");
                Err(OutputCaptureError::Read {
                    source: io::Error::other("audit stdout read failed"),
                    output: CapturedOutput {
                        bytes: b"partial stdout".to_vec(),
                        truncated: false,
                        complete: false,
                    },
                })
            }),
            cancellation,
        )
    }

    fn audit_live_child(state: Arc<AuditChildState>) -> ManagedChildProcess {
        ManagedChildProcess::new(Box::new(AuditChild::new(state)), false)
    }

    fn audit_state() -> Arc<AuditChildState> {
        Arc::new(AuditChildState {
            status: Mutex::new(None),
            changed: Condvar::new(),
            events: Mutex::new(Vec::new()),
            wait_started: Mutex::new(None),
        })
    }

    #[test]
    fn test_audit_002_read_failure_while_child_alive_with_timeout() {
        let events = audit_state();
        let (failure_tx, failure_rx) = mpsc::channel();
        let (timer_tx, timer_rx) = mpsc::channel();
        let clock = ManualMonotonicClock::new_shared();
        let timer: Arc<dyn Timer> = Arc::new(AuditTimer {
            clock: CountingClock {
                inner: Arc::clone(&clock),
                now_calls: Arc::new(AtomicUsize::new(0)),
            },
            registered: timer_tx,
        });
        let started_at = timer.clock().now();
        let watchdog_child = AuditChild::new(Arc::clone(&events));
        let running = RunningCommand::new(
            "audit 002 timeout".to_owned(),
            audit_live_child(Arc::clone(&events)),
            CommandIo::new(
                audit_read_failure_reader(failure_tx, Arc::clone(&events)),
                immediate_successful_reader(),
                None,
            ),
            started_at,
            timer,
            None,
            None,
        );
        let started = Instant::now();
        let (return_tx, return_rx) = mpsc::channel();
        let runner = thread::spawn(move || {
            let result = running.wait_for_completion(Some(Duration::from_secs(5)));
            return_tx
                .send((Instant::now(), result))
                .expect("runner result should be observed");
        });

        let failure_at = failure_rx.recv_timeout(Duration::from_secs(2));
        let timer_registered = timer_rx.recv_timeout(Duration::from_secs(2));
        let timer_advance = clock.advance(Duration::from_secs(5));
        let first_result = return_rx.recv_timeout(Duration::from_secs(2));
        let watchdog_used =
            failure_at.is_err() || timer_registered.is_err() || timer_advance.is_err() || first_result.is_err();
        if watchdog_used {
            watchdog_child.release(status(0));
            let _ = clock.advance(Duration::from_secs(5));
        }
        let runner_result = first_result
            .ok()
            .or_else(|| return_rx.recv_timeout(Duration::from_secs(2)).ok());
        runner.join().expect("runner thread should join");
        let (returned_at, result) = runner_result.expect("runner should finish after timer or watchdog release");
        let failure_at = failure_at.expect("reader should fail");
        assert!(timer_registered.is_ok(), "runner should register timeout timer");
        assert!(timer_advance.is_ok(), "manual deadline should advance");
        let error = expect_error(result, "timeout should terminate the still-live child");

        events.events.lock().expect("events should lock").push("returned");
        assert_eq!(error.kind(), CommandErrorKind::TimedOut);
        let capture = error
            .captured_output()
            .expect("timeout should retain partial reader output");
        assert_eq!(capture.stdout(), b"partial stdout");
        assert!(!capture.stdout_complete());
        assert!(returned_at >= failure_at);
        assert!(returned_at.duration_since(started) < Duration::from_secs(2));
        assert!(
            !watchdog_used,
            "controllable timeout timer should release runner without watchdog"
        );
        assert_eq!(
            *events.events.lock().expect("events should lock"),
            ["reader-failed", "kill", "wait", "returned"],
        );
        assert_eq!(error.cleanup_failures().len(), 1);
    }

    #[test]
    fn test_audit_002_read_failure_while_child_alive_without_timeout() {
        let events = audit_state();
        let (failure_tx, failure_rx) = mpsc::channel();
        let (wait_started_tx, wait_started_rx) = mpsc::channel();
        let (return_tx, return_rx) = mpsc::channel();
        *events.wait_started.lock().expect("wait-started signal should lock") = Some(wait_started_tx);
        let watchdog_child = AuditChild::new(Arc::clone(&events));
        let running = running(
            audit_live_child(Arc::clone(&events)),
            CommandIo::new(
                audit_read_failure_reader(failure_tx, Arc::clone(&events)),
                immediate_successful_reader(),
                None,
            ),
        );
        let failure_at = failure_rx.recv_timeout(Duration::from_secs(2));
        let started = Instant::now();
        let runner = thread::spawn(move || {
            let result = running.wait_for_completion(None);
            return_tx
                .send((Instant::now(), result))
                .expect("runner result should be observed");
        });

        let wait_started = wait_started_rx.recv_timeout(Duration::from_secs(2));
        let early_result = if wait_started.is_ok() {
            return_rx.recv_timeout(Duration::from_secs(2))
        } else {
            Err(mpsc::RecvTimeoutError::Timeout)
        };
        let runner_was_blocked = early_result.is_err();
        events
            .events
            .lock()
            .expect("events should lock")
            .push("watchdog-release");
        watchdog_child.release(status(0));
        let runner_result = early_result
            .ok()
            .or_else(|| return_rx.recv_timeout(Duration::from_secs(2)).ok());
        runner.join().expect("runner thread should join");
        let (returned_at, result) = runner_result.expect("watchdog should release runner");
        let failure_at = failure_at.expect("reader should fail");
        assert!(wait_started.is_ok(), "runner should enter child wait");
        let error = expect_error(result, "reader failure should be reported after child exits");

        assert!(returned_at >= failure_at);
        assert!(returned_at.duration_since(started) >= Duration::from_secs(2));
        assert_eq!(error.kind(), CommandErrorKind::ReadOutputFailed);
        assert!(
            runner_was_blocked,
            "watchdog must be the reason the no-timeout runner returned"
        );
        let capture = error
            .captured_output()
            .expect("read failure should retain partial capture");
        assert_eq!(capture.stdout(), b"partial stdout");
        assert!(!capture.stdout_complete());
        assert_eq!(
            *events.events.lock().expect("events should lock"),
            ["reader-failed", "wait", "watchdog-release"],
        );
    }

    #[test]
    fn budget_only_timer_live_child_uses_timer() {
        let child = ScriptedChild::new(raw_child()).try_wait_results([Ok(None)]);
        let (timer, at_calls) = failing_timer();
        let result = running_with_budget_and_timer(
            ManagedChildProcess::new(Box::new(child), true),
            CommandIo::new(successful_reader(), successful_reader(), None),
            Arc::new(TotalOutputBudget::new(10, None, None)),
            timer,
        )
        .wait_for_completion(None);

        let error = expect_error(result, "budget-only monitoring should consult its timer");
        assert_eq!(error.kind(), CommandErrorKind::TimeFailed);
        assert_eq!(at_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cancellation_while_waiting_for_io_retains_child_status() {
        let child = ScriptedChild::new(raw_child()).try_wait_results([Ok(Some(status(0)))]);
        let mut command = running(
            ManagedChildProcess::new(Box::new(child), true),
            CommandIo::new(successful_reader(), successful_reader(), None),
        );
        let cancellation = crate::CommandCancellation::new();
        cancellation.cancel();
        command.cancellation_token = Some(cancellation);

        let result = command.wait_for_io_event(status(0), None);

        assert!(matches!(result, RunEvent::Cancelled { status: Some(value) } if value.success()));
    }

    #[test]
    fn active_child_output_budget_precedes_ready_cancellation() {
        let child = ScriptedChild::new(raw_child()).try_wait_results([Ok(None)]);
        let mut command = running_with_budget(
            ManagedChildProcess::new(Box::new(child), true),
            CommandIo::new(successful_reader(), successful_reader(), None),
            exceeded_total_budget(),
        );
        let cancellation = crate::CommandCancellation::new();
        cancellation.cancel();
        command.cancellation_token = Some(cancellation);

        assert!(matches!(
            command.wait_for_event(None),
            RunEvent::OutputLimitExceeded {
                limit: 1,
                observed: 2,
                status: None
            }
        ));
    }

    #[test]
    fn exited_child_precedes_ready_cancellation() {
        let child = ScriptedChild::new(raw_child()).try_wait_results([Ok(Some(status(7)))]);
        let mut command = running(
            ManagedChildProcess::new(Box::new(child), true),
            CommandIo::new(successful_reader(), successful_reader(), None),
        );
        let cancellation = crate::CommandCancellation::new();
        cancellation.cancel();
        command.cancellation_token = Some(cancellation);

        assert!(matches!(command.wait_for_event(None), RunEvent::Exited(value) if value.code() == Some(7)));
    }

    #[test]
    fn post_exit_io_timeout_uses_original_started_at() {
        let clock = ManualMonotonicClock::new_shared();
        let started_at = clock.now();
        let timer = clock.new_timer();
        clock
            .advance(Duration::from_secs(2))
            .expect("manual time should advance");
        let child = ScriptedChild::new(raw_child());
        let mut command = RunningCommand::new(
            "test command".to_owned(),
            ManagedChildProcess::new(Box::new(child), true),
            CommandIo::new(successful_reader(), successful_reader(), None),
            started_at,
            timer,
            None,
            None,
        );

        assert!(matches!(
            command.wait_for_io_event(status(0), Some(Duration::from_secs(1))),
            RunEvent::TimedOut { timeout, status: Some(value) }
                if timeout == Duration::from_secs(1) && value.success()
        ));
    }

    #[test]
    fn no_timeout_monitor_does_not_sample_elapsed_time() {
        let child = ScriptedChild::new(raw_child()).try_wait_results([Ok(None), Ok(Some(status(0)))]);
        let clock = CountingClock {
            inner: ManualMonotonicClock::new_shared(),
            now_calls: Arc::new(AtomicUsize::new(0)),
        };
        let now_calls = Arc::clone(&clock.now_calls);
        let at_calls = Arc::new(AtomicUsize::new(0));
        let timer: Arc<dyn Timer> = Arc::new(ReadyCountingTimer {
            clock,
            at_calls: Arc::clone(&at_calls),
        });
        let started_at = timer.clock().now();
        let samples_before_wait = now_calls.load(Ordering::SeqCst);
        let mut command = RunningCommand::new(
            "test command".to_owned(),
            ManagedChildProcess::new(Box::new(child), true),
            CommandIo::new(successful_reader(), successful_reader(), None),
            started_at,
            timer,
            Some(crate::CommandCancellation::new()),
            None,
        );

        assert!(matches!(command.wait_for_event(None), RunEvent::Exited(value) if value.success()));
        assert_eq!(at_calls.load(Ordering::SeqCst), 1);
        assert_eq!(now_calls.load(Ordering::SeqCst), samples_before_wait + 1);
    }

    #[test]
    fn budget_only_timer_after_exit_uses_timer() {
        let child = ScriptedChild::new(raw_child()).try_wait_results([Ok(Some(status(0)))]);
        let budget = Arc::new(TotalOutputBudget::new(10, None, None));
        let (reader, _release) = blocked_budget_reader(Arc::clone(&budget));
        let (timer, at_calls) = failing_timer();
        let result = running_with_budget_and_timer(
            ManagedChildProcess::new(Box::new(child), true),
            CommandIo::new(reader, joined_stderr_reader(), None),
            budget,
            timer,
        )
        .wait_for_completion(None);

        let error = expect_error(result, "budget-only helper monitoring should consult its timer");
        assert_eq!(error.kind(), CommandErrorKind::TimeFailed);
        assert_eq!(at_calls.load(Ordering::SeqCst), 1);
    }

    fn exceeded_total_budget() -> Arc<TotalOutputBudget> {
        let budget = Arc::new(TotalOutputBudget::new(1, Some(2), Some(2)));
        let mut reader = Cursor::new(b"ab");
        let mut buffer = [0_u8; 8];
        let ready_capacity = buffer.len();
        assert_eq!(
            budget
                .read_ready(&mut reader, &mut buffer, ready_capacity, crate::OutputStream::Stdout)
                .expect("scripted ready bytes should be read"),
            2,
        );
        assert_eq!(budget.exceeded(), Some((1, 2)));
        budget
    }

    fn budget_with_stdout(limit: u64, maximum: Option<usize>, bytes: &[u8]) -> Arc<TotalOutputBudget> {
        let budget = Arc::new(TotalOutputBudget::new(limit, maximum, Some(16)));
        let mut reader = Cursor::new(bytes);
        let mut buffer = [0_u8; 32];
        let ready_capacity = buffer.len();
        budget
            .read_ready(&mut reader, &mut buffer, ready_capacity, crate::OutputStream::Stdout)
            .expect("scripted ready bytes should be read");
        budget
    }

    struct HelperRelease {
        release: Arc<AtomicBool>,
        worker_lifetime: Weak<AtomicBool>,
    }

    impl Drop for HelperRelease {
        fn drop(&mut self) {
            self.release.store(true, Ordering::Release);
            while self.worker_lifetime.upgrade().is_some() {
                thread::yield_now();
            }
        }
    }

    fn blocked_budget_reader(budget: Arc<TotalOutputBudget>) -> (OutputReader, HelperRelease) {
        let release = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let worker_lifetime = Arc::new(AtomicBool::new(false));
        let worker_release = Arc::clone(&release);
        let worker_started = Arc::clone(&started);
        let worker_lifetime_ref = Arc::clone(&worker_lifetime);
        let join = thread::spawn(move || {
            let _worker_lifetime_ref = worker_lifetime_ref;
            worker_started.store(true, Ordering::Release);
            while !worker_release.load(Ordering::Acquire) {
                thread::yield_now();
            }
            Ok(CapturedOutput {
                bytes: b"late bytes".to_vec(),
                truncated: false,
                complete: true,
            })
        });
        while !started.load(Ordering::Acquire) {
            thread::yield_now();
        }
        (
            OutputReader::new_with_budget(
                join,
                IoCancellation::failing("stdout cancellation failed"),
                budget,
                crate::OutputStream::Stdout,
            ),
            HelperRelease {
                release,
                worker_lifetime: Arc::downgrade(&worker_lifetime),
            },
        )
    }

    fn blocked_unbudgeted_reader() -> (OutputReader, HelperRelease) {
        let release = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let worker_lifetime = Arc::new(AtomicBool::new(false));
        let worker_release = Arc::clone(&release);
        let worker_started = Arc::clone(&started);
        let worker_lifetime_ref = Arc::clone(&worker_lifetime);
        let join = thread::spawn(move || {
            let _worker_lifetime_ref = worker_lifetime_ref;
            worker_started.store(true, Ordering::Release);
            while !worker_release.load(Ordering::Acquire) {
                thread::yield_now();
            }
            Ok(CapturedOutput::default())
        });
        while !started.load(Ordering::Acquire) {
            thread::yield_now();
        }
        (
            OutputReader::new(join, IoCancellation::failing("stdout cancellation failed")),
            HelperRelease {
                release,
                worker_lifetime: Arc::downgrade(&worker_lifetime),
            },
        )
    }

    fn joined_stderr_reader() -> OutputReader {
        OutputReader::new(
            thread::spawn(|| {
                Ok(CapturedOutput {
                    bytes: b"joined stderr".to_vec(),
                    truncated: false,
                    complete: true,
                })
            }),
            IoCancellation::failing("stderr cancellation failed"),
        )
    }

    fn joined_budget_reader(budget: Arc<TotalOutputBudget>) -> OutputReader {
        OutputReader::new_with_budget(
            thread::spawn(|| {
                Ok(CapturedOutput {
                    bytes: b"worker bytes must not replace budget ownership".to_vec(),
                    truncated: false,
                    complete: true,
                })
            }),
            IoCancellation::failing("stdout cancellation failed"),
            budget,
            crate::OutputStream::Stdout,
        )
    }

    fn assert_detached_budget_snapshot(
        event: RunEvent,
        budget: Arc<TotalOutputBudget>,
        expected_kind: CommandErrorKind,
        expected_stdout: &[u8],
        expected_truncated: bool,
    ) {
        let (stdout, _release) = blocked_budget_reader(Arc::clone(&budget));
        let error = expect_error(
            running_with_budget(
                terminating_child(status(0)),
                CommandIo::new(stdout, joined_stderr_reader(), None),
                budget,
            )
            .resolve_event(event, None),
            "stop reason should remain an error after helper cleanup",
        );
        assert_eq!(error.kind(), expected_kind);
        let captured = error.captured_output().expect("stop reason should retain output");
        assert_eq!(captured.stdout(), expected_stdout);
        assert!(!captured.stdout_complete());
        assert_eq!(captured.stdout_truncated(), expected_truncated);
        assert_eq!(captured.stderr(), b"joined stderr");
        assert!(captured.stderr_complete());
        assert!(
            error
                .cleanup_failures()
                .iter()
                .any(|failure| matches!(failure, CommandCleanupFailure::StdoutCancellation { .. })),
            "failed cancellation must remain in cleanup failures"
        );
    }

    #[test]
    fn test_unjoined_budget_snapshot_is_retained_for_output_limit() {
        assert_detached_budget_snapshot(
            RunEvent::OutputLimitExceeded {
                limit: 1,
                observed: 2,
                status: None,
            },
            budget_with_stdout(1, Some(1), b"ab"),
            CommandErrorKind::OutputLimitExceeded,
            b"a",
            true,
        );
    }

    #[test]
    fn test_unjoined_budget_snapshot_is_retained_for_timeout() {
        assert_detached_budget_snapshot(
            RunEvent::TimedOut {
                timeout: Duration::from_secs(2),
                status: None,
            },
            budget_with_stdout(100, Some(2), b"abc"),
            CommandErrorKind::TimedOut,
            b"ab",
            true,
        );
    }

    #[test]
    fn test_unjoined_budget_snapshot_is_retained_for_cancellation() {
        assert_detached_budget_snapshot(
            RunEvent::Cancelled { status: None },
            budget_with_stdout(100, Some(2), b"abc"),
            CommandErrorKind::Cancelled,
            b"ab",
            true,
        );
    }

    #[test]
    fn test_unjoined_budget_snapshot_preserves_zero_retention_metadata() {
        assert_detached_budget_snapshot(
            RunEvent::TimedOut {
                timeout: Duration::from_secs(2),
                status: None,
            },
            budget_with_stdout(100, Some(0), b"a"),
            CommandErrorKind::TimedOut,
            b"",
            true,
        );
    }

    #[test]
    fn test_joined_budget_output_is_transferred_once_without_detached_snapshot() {
        let budget = budget_with_stdout(1, Some(1), b"ab");
        let error = expect_error(
            running_with_budget(
                terminating_child(status(0)),
                CommandIo::new(joined_budget_reader(Arc::clone(&budget)), joined_stderr_reader(), None),
                Arc::clone(&budget),
            )
            .resolve_event(
                RunEvent::OutputLimitExceeded {
                    limit: 1,
                    observed: 2,
                    status: None,
                },
                None,
            ),
            "output budget violation should remain primary after joined collection",
        );
        assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
        let captured = error.captured_output().expect("budget error should retain output");
        assert_eq!(captured.stdout(), b"a");
        assert!(captured.stdout_truncated());
        assert!(!captured.stdout_complete());
        assert!(
            budget.captured_output().stdout().is_empty(),
            "joining should transfer the budget prefix exactly once"
        );
        assert_eq!(captured.stderr(), b"joined stderr");
    }

    #[test]
    fn test_unjoined_reader_without_budget_keeps_empty_incomplete_output() {
        let (stdout, _release) = blocked_unbudgeted_reader();
        let error = expect_error(
            running(
                terminating_child(status(0)),
                CommandIo::new(stdout, joined_stderr_reader(), None),
            )
            .resolve_event(
                RunEvent::TimedOut {
                    timeout: Duration::from_secs(2),
                    status: None,
                },
                None,
            ),
            "timeout should remain primary without a budgeted reader",
        );
        let captured = error.captured_output().expect("timeout should retain output metadata");
        assert!(captured.stdout().is_empty());
        assert!(!captured.stdout_complete());
        assert_eq!(captured.stderr(), b"joined stderr");
        assert!(captured.stderr_complete());
    }

    fn termination_failure_child() -> ManagedChildProcess {
        let direct = ScriptedChild::new(raw_child())
            .kill_error(io::Error::other("child termination failed"))
            .try_wait_results((0..9).map(|_| Ok(None)));
        let tree = ScriptedChild::new(Box::new(direct))
            .kill_error(io::Error::other("tree termination failed"))
            .try_wait_results((0..8).map(|_| Ok(None)));
        ManagedChildProcess::new(Box::new(tree), true)
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
            "timeout should remain primary after output failure",
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
                CommandCleanupFailure::Wait { .. } => "wait",
                CommandCleanupFailure::Time { .. } => "time",
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

    #[test]
    fn test_total_output_limit_remains_primary_when_helpers_fail_during_cleanup() {
        let error = expect_error(
            running_with_budget(
                terminating_child(status(0)),
                io_with_failures(),
                exceeded_total_budget(),
            )
            .resolve_event(
                RunEvent::OutputLimitExceeded {
                    limit: 1,
                    observed: 2,
                    status: None,
                },
                None,
            ),
            "output budget violation must remain primary after helper cleanup failures",
        );

        assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
        assert!(matches!(
            error.reason(),
            CommandErrorReason::OutputLimitExceeded { limit: 1, observed: 2 }
        ));
        assert!(
            error.output().is_none(),
            "a budget error must not fabricate an ExitStatus"
        );
        let captured = error
            .captured_output()
            .expect("the consumed prefix must remain available");
        assert_eq!(captured.stdout(), b"ab");
        assert!(!captured.stdout_complete());
        assert_eq!(cleanup_order(&error), ["stdout-read", "stderr-write", "stdin-write"]);
    }

    #[test]
    fn test_total_output_limit_remains_primary_when_tree_termination_fails() {
        let error = expect_error(
            running_with_budget(termination_failure_child(), io_with_failures(), exceeded_total_budget())
                .resolve_event(
                    RunEvent::OutputLimitExceeded {
                        limit: 1,
                        observed: 2,
                        status: None,
                    },
                    None,
                ),
            "output budget violation must survive process termination failure",
        );

        assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
        assert!(matches!(
            error.reason(),
            CommandErrorReason::OutputLimitExceeded { limit: 1, observed: 2 }
        ));
        let captured = error
            .captured_output()
            .expect("the captured prefix must survive failed termination");
        assert_eq!(captured.stdout(), b"ab");
        assert_eq!(
            cleanup_order(&error),
            [
                "process-tree",
                "direct-child",
                "stdout-read",
                "stderr-write",
                "stdin-write"
            ]
        );
    }

    #[derive(Clone, Copy)]
    enum ExpectedReason {
        TimedOut,
        Cancelled,
        TimeFailed,
        WaitFailed,
    }

    #[test]
    fn test_resolve_event_preserves_error_priority_and_cleanup_order() {
        let cases = [
            (
                "timeout plus output read failure",
                timeout_with_output_failure as fn() -> CommandError,
                CommandErrorKind::TimedOut,
                ExpectedReason::TimedOut,
                true,
                &["stdout-read"][..],
            ),
            (
                "cancellation plus tree fallback and helper failures",
                cancellation_with_fallback_and_helper_failures,
                CommandErrorKind::Cancelled,
                ExpectedReason::Cancelled,
                false,
                &[
                    "process-tree",
                    "direct-child",
                    "time",
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
                true,
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
                    (CommandErrorReason::TimedOut { .. }, ExpectedReason::TimedOut)
                        | (CommandErrorReason::Cancelled, ExpectedReason::Cancelled)
                        | (CommandErrorReason::TimeFailed { .. }, ExpectedReason::TimeFailed)
                        | (CommandErrorReason::WaitFailed { .. }, ExpectedReason::WaitFailed)
                ),
                "{name}"
            );
            assert_eq!(error.output().is_some(), has_output, "{name}");
            assert_eq!(cleanup_order(&error), expected_cleanup, "{name}");
        }
    }
    #[test]
    fn test_resolve_event_preserves_all_stop_reasons_across_cleanup_outcomes() {
        use std::sync::Arc;
        use std::sync::Mutex;

        use super::super::scripted_child::ScriptedChild as RecordingChild;
        for event_kind in 0..7 {
            for termination_mode in 0..3 {
                let calls = Arc::new(Mutex::new(Vec::new()));
                let child = if termination_mode == 1 {
                    let direct = RecordingChild::new("child", raw_child(), Arc::clone(&calls))
                        .kill_error(io::Error::other("child denied"));
                    let tree = RecordingChild::new("tree", Box::new(direct), calls)
                        .kill_error(io::Error::other("tree denied"))
                        .try_wait_results((0..9).map(|_| Ok(None)));
                    ManagedChildProcess::new(Box::new(tree), true)
                } else if termination_mode == 2 {
                    let tree = RecordingChild::new("tree", raw_child(), calls)
                        .wait_error(io::Error::other("final wait failed"));
                    ManagedChildProcess::new(Box::new(tree), true)
                } else {
                    terminating_child(status(0))
                };
                let (event, expected) = match event_kind {
                    0 => (
                        RunEvent::TimedOut {
                            timeout: Duration::from_secs(2),
                            status: None,
                        },
                        CommandErrorKind::TimedOut,
                    ),
                    1 => (RunEvent::Cancelled { status: None }, CommandErrorKind::Cancelled),
                    2 => (
                        RunEvent::WaitFailed(io::Error::other("initial wait failed")),
                        CommandErrorKind::WaitFailed,
                    ),
                    3 => (
                        RunEvent::TimeFailed {
                            source: TimeError::InstantOverflow,
                            status: None,
                        },
                        CommandErrorKind::TimeFailed,
                    ),
                    4 => (
                        RunEvent::TimedOut {
                            timeout: Duration::from_secs(2),
                            status: Some(status(0)),
                        },
                        CommandErrorKind::TimedOut,
                    ),
                    5 => (
                        RunEvent::Cancelled {
                            status: Some(status(0)),
                        },
                        CommandErrorKind::Cancelled,
                    ),
                    _ => (
                        RunEvent::TimeFailed {
                            source: TimeError::InstantOverflow,
                            status: Some(status(0)),
                        },
                        CommandErrorKind::TimeFailed,
                    ),
                };
                let error = expect_error(
                    running(child, io_with_failures()).resolve_event(event, None),
                    "a stopped command must remain an error",
                );
                assert_eq!(
                    error.kind(),
                    expected,
                    "event {event_kind}, termination mode {termination_mode}"
                );
                let cleanup = cleanup_order(&error);
                assert!(cleanup.ends_with(&["stdout-read", "stderr-write", "stdin-write"]));
                assert_eq!(error.process_tree_source().is_some(), termination_mode == 1);
                assert_eq!(error.child_source().is_some(), termination_mode == 1 && event_kind < 4);
                assert_eq!(cleanup.contains(&"wait"), termination_mode == 2 && event_kind < 4);
                assert_eq!(
                    error.output().is_some(),
                    event_kind >= 4 || (termination_mode == 0 && event_kind < 2)
                );
            }
        }
    }

    #[test]
    fn test_timeout_retains_each_individual_helper_failure() {
        for failed_stream in 0..3 {
            let stdout = if failed_stream == 0 {
                failing_stdout()
            } else {
                successful_reader()
            };
            let stderr = if failed_stream == 1 {
                failing_stderr()
            } else {
                successful_reader()
            };
            let stdin = if failed_stream == 2 {
                Some(completed_writer(Err(io::Error::other("stdin failed"))))
            } else {
                None
            };
            let error = expect_error(
                running(terminating_child(status(0)), CommandIo::new(stdout, stderr, stdin)).resolve_event(
                    RunEvent::TimedOut {
                        timeout: Duration::from_secs(2),
                        status: None,
                    },
                    None,
                ),
                "timeout must remain primary",
            );
            assert_eq!(error.kind(), CommandErrorKind::TimedOut);
            assert!(error.output().is_some());
            assert_eq!(
                cleanup_order(&error),
                [match failed_stream {
                    0 => "stdout-read",
                    1 => "stderr-write",
                    _ => "stdin-write",
                }]
            );
        }
    }
}
