// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    io,
    process::ExitStatus,
    sync::Arc,
    thread,
    time::Duration,
};

use qubit_clock::{
    BlockingSleeper,
    MonotonicInstant,
    TimeError,
    Timer,
};

use super::{
    command_io::CommandIo,
    error_mapping::{
        kill_failed,
        wait_failed,
    },
    finished_command::FinishedCommand,
    managed_child_process::ManagedChildProcess,
    process_termination_error::ProcessTerminationError,
    wait_policy::next_sleep,
};
use crate::{
    CommandCancellation,
    CommandError,
};

/// Maximum delay before a cancellation-aware wait observes cancellation.
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Bounded confirmation window for a child racing process-tree termination.
const KILL_FAILURE_EXIT_CHECK_ATTEMPTS: usize = 8;
/// Delay between bounded exit checks after a failed process-tree kill.
const KILL_FAILURE_EXIT_CHECK_DELAY: Duration = Duration::from_micros(50);

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
    /// output collection, or stdin writing fails. Wait-error cleanup only
    /// joins I/O helpers after a non-blocking check confirms the child has
    /// exited.
    pub(in crate::command_runner) fn wait_for_completion(
        mut self,
        timeout: Option<Duration>,
    ) -> Result<FinishedCommand, CommandError> {
        if timeout.is_none() && self.cancellation_token.is_none() {
            let status = match self.child_process.wait() {
                Ok(status) => status,
                Err(source) => {
                    let error = wait_failed(&self.command_text, source);
                    return Err(self.clean_up_after_wait_error(error));
                }
            };
            return self.complete_after_exit(status, None);
        }

        let mut timeout_poll_count = 0;
        loop {
            let maybe_status = match self.child_process.try_wait() {
                Ok(status) => status,
                Err(source) => {
                    let error = wait_failed(&self.command_text, source);
                    return Err(self.clean_up_after_wait_error(error));
                }
            };
            if let Some(status) = maybe_status {
                return self.complete_after_exit(status, timeout);
            }
            if self
                .cancellation_token
                .as_ref()
                .is_some_and(CommandCancellation::is_cancelled)
            {
                return self.handle_cancellation();
            }
            let sleep = match timeout {
                Some(timeout) => {
                    let elapsed = match self.elapsed() {
                        Ok(elapsed) => elapsed,
                        Err(source) => {
                            return Err(self.clean_up_after_time_error(source));
                        }
                    };
                    if elapsed >= timeout {
                        return self.handle_timeout(timeout);
                    }
                    let sleep =
                        next_sleep(timeout, elapsed, timeout_poll_count);
                    timeout_poll_count = timeout_poll_count.saturating_add(1);
                    sleep
                }
                None => CANCELLATION_POLL_INTERVAL,
            };
            if let Err(source) =
                BlockingSleeper::new(Arc::clone(&self.timer)).sleep_for(sleep)
            {
                return Err(self.clean_up_after_time_error(source));
            }
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
    /// Returns [`CommandError::TimedOut`] or [`CommandError::Cancelled`] when
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
                    return self.handle_output_collection_cancellation(status);
                }
                let sleep = match timeout {
                    Some(timeout) => {
                        let elapsed = match self.elapsed() {
                            Ok(elapsed) => elapsed,
                            Err(source) => {
                                return self.handle_time_error_after_exit(
                                    status, source,
                                );
                            }
                        };
                        if elapsed >= timeout {
                            return self.handle_output_collection_timeout(
                                status, timeout,
                            );
                        }
                        let sleep =
                            next_sleep(timeout, elapsed, timeout_poll_count);
                        timeout_poll_count =
                            timeout_poll_count.saturating_add(1);
                        sleep
                    }
                    None => CANCELLATION_POLL_INTERVAL,
                };
                if let Err(source) =
                    BlockingSleeper::new(Arc::clone(&self.timer))
                        .sleep_for(sleep)
                {
                    return self.handle_time_error_after_exit(status, source);
                }
            }
        }
        self.complete(status)
    }

    /// Handles timeout reached while collecting inherited output pipes.
    ///
    /// # Parameters
    ///
    /// * `status` - Exit status reported by the direct child process.
    /// * `timeout` - Timeout that has been exceeded.
    ///
    /// # Returns
    ///
    /// This method returns an error after timeout handling; its success type is
    /// retained to compose with the surrounding state machine.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::TimedOut`] after terminating the process tree
    /// and collecting final output, or the process-control / collection
    /// error that prevented timeout output from being built.
    fn handle_output_collection_timeout(
        mut self,
        status: ExitStatus,
        timeout: Duration,
    ) -> Result<FinishedCommand, CommandError> {
        if let Err(source) = self.child_process.start_kill()
            && !Self::process_tree_already_exited(&source)
        {
            return Err(kill_failed(
                self.command_text.clone(),
                timeout,
                source,
            ));
        }
        let finished = self.complete(status)?;
        Err(CommandError::TimedOut {
            command: finished.command_text,
            timeout,
            output: Box::new(finished.output),
        })
    }

    /// Cancels descendants that keep inherited I/O pipes open after the
    /// direct child has exited.
    ///
    /// # Parameters
    ///
    /// * `status` - Exit status reported for the direct child.
    ///
    /// # Returns
    ///
    /// Always returns a cancellation or process-control error after cleanup.
    fn handle_output_collection_cancellation(
        mut self,
        status: ExitStatus,
    ) -> Result<FinishedCommand, CommandError> {
        if let Err(source) = self.child_process.start_kill()
            && !Self::process_tree_already_exited(&source)
        {
            return Err(CommandError::CancelFailed {
                command: self.command_text.clone(),
                source,
            });
        }
        let finished = self.complete(status)?;
        Err(CommandError::Cancelled {
            command: finished.command_text,
            output: Box::new(finished.output),
        })
    }

    /// Cancels a running process tree and collects its final output.
    ///
    /// # Returns
    ///
    /// Always returns a cancellation or process-control error after cleanup.
    fn handle_cancellation(mut self) -> Result<FinishedCommand, CommandError> {
        let status = match self.terminate_child() {
            Ok(status) => status,
            Err(ProcessTerminationError::Wait(source)) => {
                let error = wait_failed(&self.command_text, source);
                return Err(self.collect_if_child_exited(error));
            }
            Err(ProcessTerminationError::Kill(source)) => {
                return Err(CommandError::CancelFailed {
                    command: self.command_text.clone(),
                    source,
                });
            }
        };
        let finished = self.complete(status)?;
        Err(CommandError::Cancelled {
            command: finished.command_text,
            output: Box::new(finished.output),
        })
    }

    /// Handles timeout by killing the child process and collecting final
    /// output.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Timeout that has been exceeded.
    ///
    /// # Returns
    ///
    /// This method returns an error after timeout handling; its success type is
    /// retained to compose with the surrounding state machine.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::TimedOut`] after successful kill and wait, or
    /// the process-control error if killing or waiting fails. Cleanup after
    /// those errors only joins I/O helpers if the child is already
    /// confirmed exited.
    fn handle_timeout(
        mut self,
        timeout: Duration,
    ) -> Result<FinishedCommand, CommandError> {
        let exit_status = match self.terminate_child() {
            Ok(status) => status,
            Err(ProcessTerminationError::Wait(source)) => {
                let error = wait_failed(&self.command_text, source);
                return Err(self.collect_if_child_exited(error));
            }
            Err(ProcessTerminationError::Kill(source)) => {
                let error =
                    kill_failed(self.command_text.clone(), timeout, source);
                return Err(self.collect_if_child_exited(error));
            }
        };
        let finished = self.complete(exit_status)?;
        Err(CommandError::TimedOut {
            command: finished.command_text,
            timeout,
            output: Box::new(finished.output),
        })
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
    fn complete(
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
        let output = io.collect(&command_text, status, move || {
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

    /// Terminates the managed process tree and waits for the direct child.
    ///
    /// A failed termination request is treated as successful when the direct
    /// child is concurrently observed to have exited.
    ///
    /// # Returns
    ///
    /// Final direct-child status after termination or a concurrent exit.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessTerminationError::Wait`] when waiting after a
    /// successful termination request fails. Returns
    /// [`ProcessTerminationError::Kill`] when termination fails and the child
    /// cannot be confirmed exited.
    fn terminate_child(
        &mut self,
    ) -> Result<ExitStatus, ProcessTerminationError> {
        match self.child_process.start_kill() {
            Ok(()) => self
                .child_process
                .wait()
                .map_err(ProcessTerminationError::Wait),
            Err(source) => match self.status_after_kill_failure(&source) {
                Ok(Some(status)) => Ok(status),
                Ok(None) | Err(_) => Err(ProcessTerminationError::Kill(source)),
            },
        }
    }

    /// Resolves the direct-child status after process-tree termination fails.
    ///
    /// # Parameters
    ///
    /// * `source` - Process-tree termination error.
    ///
    /// # Returns
    ///
    /// The completed direct-child status when it can be observed, or `None`
    /// when the child remains running.
    ///
    /// # Errors
    ///
    /// Returns the operating-system wait error when the final child status
    /// cannot be observed.
    fn status_after_kill_failure(
        &mut self,
        source: &io::Error,
    ) -> io::Result<Option<ExitStatus>> {
        if Self::process_tree_already_exited(source) {
            let status = self.child_process.wait();
            return status.map(Some);
        }
        for attempt in 0..KILL_FAILURE_EXIT_CHECK_ATTEMPTS {
            if let Some(status) = self.child_process.try_wait()? {
                return Ok(Some(status));
            }
            if attempt + 1 < KILL_FAILURE_EXIT_CHECK_ATTEMPTS {
                thread::sleep(KILL_FAILURE_EXIT_CHECK_DELAY);
            }
        }
        Ok(None)
    }

    /// Reports whether a process-tree termination error means the tree ended.
    ///
    /// # Parameters
    ///
    /// * `source` - Process-tree termination error.
    ///
    /// # Returns
    ///
    /// `true` when the platform reports that the managed process tree no
    /// longer exists, otherwise `false`.
    fn process_tree_already_exited(source: &io::Error) -> bool {
        #[cfg(unix)]
        {
            source.raw_os_error() == Some(libc::ESRCH)
        }
        #[cfg(not(unix))]
        {
            source.kind() == io::ErrorKind::NotFound
        }
    }

    /// Terminates a running child after timer handling fails.
    ///
    /// # Parameters
    ///
    /// * `source` - Timer or monotonic-clock failure to preserve.
    ///
    /// # Returns
    ///
    /// The timer error after best-effort process and I/O cleanup.
    #[must_use]
    fn clean_up_after_time_error(mut self, source: TimeError) -> CommandError {
        let error = CommandError::TimeFailed {
            command: self.command_text.clone(),
            source,
        };
        let status = match self.child_process.start_kill() {
            Ok(()) => self.child_process.wait().ok(),
            Err(_) => self.child_process.try_wait().ok().flatten(),
        };
        if let Some(status) = status {
            let _ = self.io.collect(&self.command_text, status, || {
                Ok::<Duration, TimeError>(Duration::ZERO)
            });
        }
        error
    }

    /// Cleans up inherited output pipes after timer handling fails.
    ///
    /// # Parameters
    ///
    /// * `status` - Exit status already reported for the direct child.
    /// * `source` - Timer or monotonic-clock failure to preserve.
    ///
    /// # Returns
    ///
    /// This method always returns the preserved time error.
    ///
    /// # Errors
    ///
    /// Always returns [`CommandError::TimeFailed`] after best-effort cleanup.
    fn handle_time_error_after_exit(
        mut self,
        status: ExitStatus,
        source: TimeError,
    ) -> Result<FinishedCommand, CommandError> {
        let error = CommandError::TimeFailed {
            command: self.command_text.clone(),
            source,
        };
        let _ = self.child_process.start_kill();
        let _ = self.io.collect(&self.command_text, status, || {
            Ok::<Duration, TimeError>(Duration::ZERO)
        });
        Err(error)
    }

    /// Attempts non-blocking cleanup after a wait error.
    ///
    /// # Parameters
    ///
    /// * `error` - Original wait error to preserve.
    ///
    /// # Returns
    ///
    /// The original error after best-effort cleanup. This method deliberately
    /// does not call blocking wait APIs because it is already handling a
    /// wait failure.
    #[must_use]
    fn clean_up_after_wait_error(
        mut self,
        error: CommandError,
    ) -> CommandError {
        let _ = self.child_process.start_kill();
        self.collect_if_child_exited(error)
    }

    /// Drains I/O helpers if the child is already known to have exited.
    ///
    /// # Parameters
    ///
    /// * `error` - Original process-control error to preserve.
    ///
    /// # Returns
    ///
    /// The original error. Output collection failures during cleanup are
    /// ignored so the primary process-control failure remains visible.
    #[must_use]
    fn collect_if_child_exited(mut self, error: CommandError) -> CommandError {
        if let Ok(Some(status)) = self.child_process.try_wait() {
            let _ = self.complete(status);
        }
        error
    }
}
