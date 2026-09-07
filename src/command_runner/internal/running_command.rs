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
        if timeout.is_none() && self.cancellation_token.is_none() {
            let event = match self.child_process.wait() {
                Ok(status) => RunEvent::Exited(status),
                Err(source) => RunEvent::WaitFailed(source),
            };
            return self.finish_run_event(event, None);
        }

        let mut timeout_poll_count = 0;
        loop {
            let maybe_status = match self.child_process.try_wait() {
                Ok(status) => status,
                Err(source) => {
                    return self.finish_run_event(RunEvent::WaitFailed(source), timeout);
                }
            };
            if let Some(status) = maybe_status {
                return self.finish_run_event(RunEvent::Exited(status), timeout);
            }
            if self
                .cancellation_token
                .as_ref()
                .is_some_and(CommandCancellation::is_cancelled)
            {
                return self.finish_run_event(RunEvent::Cancelled { status: None }, timeout);
            }
            let sleep = match timeout {
                Some(timeout) => {
                    let elapsed = match self.elapsed() {
                        Ok(elapsed) => elapsed,
                        Err(source) => {
                            return self.finish_run_event(
                                RunEvent::TimeFailed {
                                    source,
                                    status: None,
                                },
                                timeout.into(),
                            );
                        }
                    };
                    if elapsed >= timeout {
                        return self.finish_run_event(
                            RunEvent::TimedOut {
                                timeout,
                                status: None,
                            },
                            Some(timeout),
                        );
                    }
                    let sleep = next_sleep(timeout, elapsed, timeout_poll_count);
                    timeout_poll_count = timeout_poll_count.saturating_add(1);
                    sleep
                }
                None => CANCELLATION_POLL_INTERVAL,
            };
            if let Err(source) = BlockingSleeper::new(Arc::clone(&self.timer)).sleep_for(sleep) {
                return self.finish_run_event(
                    RunEvent::TimeFailed {
                        source,
                        status: None,
                    },
                    timeout,
                );
            }
        }
    }

    /// Finalizes one terminal event from the process monitoring loop.
    fn finish_run_event(
        self,
        event: RunEvent,
        timeout: Option<Duration>,
    ) -> Result<FinishedCommand, CommandError> {
        match event.into_exit_status() {
            Ok(status) => self.complete_after_exit(status, timeout),
            Err(reason) => self.stop(reason),
        }
    }

    /// Completes a command after the direct child exits.
    ///
    /// # Parameters
    ///
    /// * `status` - Exit status reported by the direct child process.
    /// * `timeout` - Optional command timeout that also bounds I/O collection.
    ///
    /// # Returns
    ///
    /// Finished command output when all I/O helpers finish before timeout or
    /// cancellation.
    ///
    /// # Errors
    ///
    /// Returns a [`CommandError`] with kind `TimedOut` or `Cancelled` when
    /// inherited I/O pipes keep helpers alive after the corresponding request,
    /// or another [`CommandError`] if cleanup or output collection fails.
    fn complete_after_exit(
        self,
        status: ExitStatus,
        timeout: Option<Duration>,
    ) -> Result<FinishedCommand, CommandError> {
        if timeout.is_some() || self.cancellation_token.is_some() {
            let mut timeout_poll_count = 0;
            while !self.io.is_finished() {
                if self
                    .cancellation_token
                    .as_ref()
                    .is_some_and(CommandCancellation::is_cancelled)
                {
                    return self.finish_run_event(
                        RunEvent::Cancelled {
                            status: Some(status),
                        },
                        timeout,
                    );
                }
                let sleep = match timeout {
                    Some(timeout) => {
                        let elapsed = match self.elapsed() {
                            Ok(elapsed) => elapsed,
                            Err(source) => {
                                return self.finish_run_event(
                                    RunEvent::TimeFailed {
                                        source,
                                        status: Some(status),
                                    },
                                    Some(timeout),
                                );
                            }
                        };
                        if elapsed >= timeout {
                            return self.finish_run_event(
                                RunEvent::TimedOut {
                                    timeout,
                                    status: Some(status),
                                },
                                Some(timeout),
                            );
                        }
                        let sleep = next_sleep(timeout, elapsed, timeout_poll_count);
                        timeout_poll_count = timeout_poll_count.saturating_add(1);
                        sleep
                    }
                    None => CANCELLATION_POLL_INTERVAL,
                };
                if let Err(source) = BlockingSleeper::new(Arc::clone(&self.timer)).sleep_for(sleep)
                {
                    return self.finish_run_event(
                        RunEvent::TimeFailed {
                            source,
                            status: Some(status),
                        },
                        timeout,
                    );
                }
            }
        }
        self.complete(status)
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
    fn stop(mut self, reason: StopReason) -> Result<FinishedCommand, CommandError> {
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
        let finished = match self.complete_after_termination(status) {
            Ok(finished) => finished,
            Err(error) => {
                return Err(error.with_cleanup_failures(outcome.cleanup_failures));
            }
        };
        Err(reason
            .into_primary_error(finished.command_text, Some(Box::new(finished.output)))
            .with_cleanup_failures(outcome.cleanup_failures))
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
    ) -> Result<FinishedCommand, CommandError> {
        let Self {
            command_text,
            io,
            started_at,
            timer,
            ..
        } = self;
        let output = io.cancel_and_collect(&command_text, status, move || {
            timer.clock().now().duration_since(started_at)
        })?;
        Ok(FinishedCommand {
            command_text,
            output,
        })
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
