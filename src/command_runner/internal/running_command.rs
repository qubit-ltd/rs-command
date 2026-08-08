// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;
use std::process::ExitStatus;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use qubit_clock::BlockingSleeper;
use qubit_clock::MonotonicInstant;
use qubit_clock::TimeError;
use qubit_clock::Timer;

use super::command_io::CommandIo;
use super::error_mapping::kill_failed;
use super::error_mapping::wait_failed;
use super::finished_command::FinishedCommand;
use super::managed_child_process::ManagedChildProcess;
use super::process_termination_error::ProcessTerminationError;
use super::process_termination_error::ProcessTerminationOutcome;
use super::wait_policy::next_sleep;
use crate::CommandCancellation;
use crate::CommandCleanupFailure;
use crate::CommandError;
use crate::CommandErrorReason;

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
    /// output collection, or stdin writing fails.
    pub(in crate::command_runner) fn wait_for_completion(
        mut self,
        timeout: Option<Duration>,
    ) -> Result<FinishedCommand, CommandError> {
        if timeout.is_none() && self.cancellation_token.is_none() {
            let status = match self.child_process.wait() {
                Ok(status) => status,
                Err(source) => {
                    let error = wait_failed(&self.command_text, source);
                    return Err(self.collect_after_wait_error(error));
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
                    return Err(self.collect_after_wait_error(error));
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
                    return self.handle_output_collection_cancellation();
                }
                let sleep = match timeout {
                    Some(timeout) => {
                        let elapsed = match self.elapsed() {
                            Ok(elapsed) => elapsed,
                            Err(source) => {
                                return self
                                    .handle_time_error_after_exit(source);
                            }
                        };
                        if elapsed >= timeout {
                            return self
                                .handle_output_collection_timeout(timeout);
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
                    return self.handle_time_error_after_exit(source);
                }
            }
        }
        self.complete(status)
    }

    /// Handles timeout reached while collecting inherited output pipes.
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
    /// Returns a [`CommandError`] with kind `TimedOut` after terminating the
    /// command and collecting final output, or the process-control/output
    /// error that prevented timeout output from being built.
    fn handle_output_collection_timeout(
        mut self,
        timeout: Duration,
    ) -> Result<FinishedCommand, CommandError> {
        let outcome = match self.terminate_child() {
            Ok(outcome) => outcome,
            Err(ProcessTerminationError::Wait(source)) => {
                let error = wait_failed(&self.command_text, source);
                return Err(self.finish_without_status(error));
            }
            Err(ProcessTerminationError::WaitAfterTreeTermination {
                wait_source,
                process_tree_source,
            }) => {
                let error = wait_failed(&self.command_text, wait_source)
                    .with_cleanup_failures([
                        CommandCleanupFailure::ProcessTreeTermination {
                            source: process_tree_source,
                        },
                    ]);
                return Err(self.finish_without_status(error));
            }
            Err(ProcessTerminationError::Kill(
                process_tree_source,
                child_source,
            )) => {
                let error = kill_failed(
                    self.command_text.clone(),
                    timeout,
                    process_tree_source,
                    child_source,
                );
                return Err(self.finish_without_status(error));
            }
        };
        let ProcessTerminationOutcome {
            status,
            cleanup_failures,
        } = outcome;
        let finished = match self.complete_after_termination(status) {
            Ok(finished) => finished,
            Err(error) => {
                return Err(error.with_cleanup_failures(cleanup_failures));
            }
        };
        Err(CommandError::from_reason(
            finished.command_text,
            CommandErrorReason::TimedOut { timeout },
            Some(Box::new(finished.output)),
        )
        .with_cleanup_failures(cleanup_failures))
    }

    /// Cancels descendants that keep inherited output pipes open after the
    /// direct child has exited.
    ///
    /// # Returns
    ///
    /// Always returns a cancellation or process-control error after cleanup.
    fn handle_output_collection_cancellation(
        mut self,
    ) -> Result<FinishedCommand, CommandError> {
        let outcome = match self.terminate_child() {
            Ok(outcome) => outcome,
            Err(ProcessTerminationError::Wait(source)) => {
                let error = wait_failed(&self.command_text, source);
                return Err(self.finish_without_status(error));
            }
            Err(ProcessTerminationError::WaitAfterTreeTermination {
                wait_source,
                process_tree_source,
            }) => {
                let error = wait_failed(&self.command_text, wait_source)
                    .with_cleanup_failures([
                        CommandCleanupFailure::ProcessTreeTermination {
                            source: process_tree_source,
                        },
                    ]);
                return Err(self.finish_without_status(error));
            }
            Err(ProcessTerminationError::Kill(
                process_tree_source,
                child_source,
            )) => {
                let error = CommandError::from_reason(
                    self.command_text.clone(),
                    CommandErrorReason::CancelFailed {
                        process_tree_source,
                        child_source,
                    },
                    None,
                );
                return Err(self.finish_without_status(error));
            }
        };
        let ProcessTerminationOutcome {
            status,
            cleanup_failures,
        } = outcome;
        let finished = match self.complete_after_termination(status) {
            Ok(finished) => finished,
            Err(error) => {
                return Err(error.with_cleanup_failures(cleanup_failures));
            }
        };
        Err(CommandError::from_reason(
            finished.command_text,
            CommandErrorReason::Cancelled,
            Some(Box::new(finished.output)),
        )
        .with_cleanup_failures(cleanup_failures))
    }

    /// Cancels a running process tree and collects its final output.
    ///
    /// # Returns
    ///
    /// Always returns a cancellation or process-control error after cleanup.
    fn handle_cancellation(mut self) -> Result<FinishedCommand, CommandError> {
        let outcome = match self.terminate_child() {
            Ok(outcome) => outcome,
            Err(ProcessTerminationError::Wait(source)) => {
                let error = wait_failed(&self.command_text, source);
                return Err(self.collect_after_wait_error(error));
            }
            Err(ProcessTerminationError::WaitAfterTreeTermination {
                wait_source,
                process_tree_source,
            }) => {
                let error = wait_failed(&self.command_text, wait_source)
                    .with_cleanup_failures([
                        CommandCleanupFailure::ProcessTreeTermination {
                            source: process_tree_source,
                        },
                    ]);
                return Err(self.finish_without_status(error));
            }
            Err(ProcessTerminationError::Kill(
                process_tree_source,
                child_source,
            )) => {
                let error = CommandError::from_reason(
                    self.command_text.clone(),
                    CommandErrorReason::CancelFailed {
                        process_tree_source,
                        child_source,
                    },
                    None,
                );
                return Err(self.finish_without_status(error));
            }
        };
        let ProcessTerminationOutcome {
            status,
            cleanup_failures,
        } = outcome;
        let finished = match self.complete_after_termination(status) {
            Ok(finished) => finished,
            Err(error) => {
                return Err(error.with_cleanup_failures(cleanup_failures));
            }
        };
        Err(CommandError::from_reason(
            finished.command_text,
            CommandErrorReason::Cancelled,
            Some(Box::new(finished.output)),
        )
        .with_cleanup_failures(cleanup_failures))
    }

    /// Handles timeout by killing the command and collecting final output.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Timeout that has been exceeded.
    ///
    /// # Returns
    ///
    /// This method returns an error after timeout handling.
    ///
    /// # Errors
    ///
    /// Returns a [`CommandError`] with kind `TimedOut` after successful
    /// termination and collection, or the process-control error from failed
    /// cleanup.
    fn handle_timeout(
        mut self,
        timeout: Duration,
    ) -> Result<FinishedCommand, CommandError> {
        let outcome = match self.terminate_child() {
            Ok(outcome) => outcome,
            Err(ProcessTerminationError::Wait(source)) => {
                let error = wait_failed(&self.command_text, source);
                return Err(self.collect_after_wait_error(error));
            }
            Err(ProcessTerminationError::WaitAfterTreeTermination {
                wait_source,
                process_tree_source,
            }) => {
                let error = wait_failed(&self.command_text, wait_source)
                    .with_cleanup_failures([
                        CommandCleanupFailure::ProcessTreeTermination {
                            source: process_tree_source,
                        },
                    ]);
                return Err(self.finish_without_status(error));
            }
            Err(ProcessTerminationError::Kill(
                process_tree_source,
                child_source,
            )) => {
                let error = kill_failed(
                    self.command_text.clone(),
                    timeout,
                    process_tree_source,
                    child_source,
                );
                return Err(self.collect_after_status_lost(error));
            }
        };
        let ProcessTerminationOutcome {
            status,
            cleanup_failures,
        } = outcome;
        let finished = match self.complete_after_termination(status) {
            Ok(finished) => finished,
            Err(error) => {
                return Err(error.with_cleanup_failures(cleanup_failures));
            }
        };
        Err(CommandError::from_reason(
            finished.command_text,
            CommandErrorReason::TimedOut { timeout },
            Some(Box::new(finished.output)),
        )
        .with_cleanup_failures(cleanup_failures))
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
        let output =
            io.cancel_and_collect(&command_text, status, move || {
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

    /// Terminates the managed process tree and returns final child status.
    ///
    /// For process-tree managed children this method first tries wrapper tree
    /// termination and falls back to direct-child kill through
    /// `inner_mut().start_kill()` when needed.
    fn terminate_child(
        &mut self,
    ) -> Result<ProcessTerminationOutcome, ProcessTerminationError> {
        if !self.child_process.process_tree_managed() {
            if let Err(child_source) = self.child_process.start_kill_child() {
                let status = self
                    .child_process
                    .try_wait()
                    .map_err(ProcessTerminationError::Wait)?;
                if let Some(status) = status {
                    return Ok(ProcessTerminationOutcome {
                        status,
                        cleanup_failures: Vec::new(),
                    });
                }
                return Err(ProcessTerminationError::Kill(
                    io::Error::other(
                        "direct kill used without tree management",
                    ),
                    child_source,
                ));
            }
            return self
                .child_process
                .wait()
                .map(|status| ProcessTerminationOutcome {
                    status,
                    cleanup_failures: Vec::new(),
                })
                .map_err(ProcessTerminationError::Wait);
        }

        match self.child_process.start_kill_tree() {
            Ok(()) => self
                .child_process
                .wait()
                .map(|status| ProcessTerminationOutcome {
                    status,
                    cleanup_failures: Vec::new(),
                })
                .map_err(ProcessTerminationError::Wait),
            Err(process_tree_source) => {
                match self
                    .status_after_termination_failure(&process_tree_source)
                {
                    Ok(Some(status)) => Ok(ProcessTerminationOutcome {
                        status,
                        cleanup_failures: if Self::process_tree_already_exited(
                            &process_tree_source,
                        ) {
                            Vec::new()
                        } else {
                            vec![CommandCleanupFailure::ProcessTreeTermination {
                                source: process_tree_source,
                            }]
                        },
                    }),
                    Ok(None) => match self.child_process.start_kill_child() {
                        Ok(()) => {
                            match self.child_process.wait() {
                                Ok(status) => Ok(ProcessTerminationOutcome {
                                    status,
                                    cleanup_failures: vec![
                                        CommandCleanupFailure::ProcessTreeTermination {
                                            source: process_tree_source,
                                        },
                                    ],
                                }),
                                Err(wait_source) => {
                                    Err(ProcessTerminationError::WaitAfterTreeTermination {
                                        wait_source,
                                        process_tree_source,
                                    })
                                }
                            }
                        }
                        Err(child_source) => {
                            let status = self
                                .child_process
                                .try_wait()
                                .map_err(ProcessTerminationError::Wait)?;
                            if let Some(status) = status {
                                Ok(ProcessTerminationOutcome {
                                    status,
                                    cleanup_failures: vec![
                                        CommandCleanupFailure::ProcessTreeTermination {
                                            source: process_tree_source,
                                        },
                                        CommandCleanupFailure::ChildTermination {
                                            source: child_source,
                                        },
                                    ],
                                })
                            } else {
                                Err(ProcessTerminationError::Kill(
                                    process_tree_source,
                                    child_source,
                                ))
                            }
                        }
                    },
                    Err(wait_source) => {
                        Err(ProcessTerminationError::WaitAfterTreeTermination {
                            wait_source,
                            process_tree_source,
                        })
                    }
                }
            }
        }
    }

    /// Resolves status after process-tree termination failure.
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
    fn status_after_termination_failure(
        &mut self,
        source: &io::Error,
    ) -> io::Result<Option<ExitStatus>> {
        if Self::process_tree_already_exited(source) {
            return self.child_process.wait().map(Some);
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

    /// Best-effort cleanup after timer failure with best-effort helper cleanup.
    ///
    /// # Parameters
    ///
    /// * `source` - Timer or monotonic-clock failure to preserve.
    ///
    /// # Returns
    ///
    /// The preserved time error after helper cleanup.
    #[must_use]
    fn clean_up_after_time_error(mut self, source: TimeError) -> CommandError {
        let error = CommandError::from_reason(
            self.command_text.clone(),
            CommandErrorReason::TimeFailed { source },
            None,
        );
        let error = match self.terminate_child() {
            Ok(outcome) => {
                error.with_cleanup_failures(outcome.cleanup_failures)
            }
            Err(ProcessTerminationError::Wait(source)) => error
                .with_cleanup_failures([CommandCleanupFailure::Wait {
                    source,
                }]),
            Err(ProcessTerminationError::WaitAfterTreeTermination {
                wait_source,
                process_tree_source,
            }) => error.with_cleanup_failures([
                CommandCleanupFailure::Wait {
                    source: wait_source,
                },
                CommandCleanupFailure::ProcessTreeTermination {
                    source: process_tree_source,
                },
            ]),
            Err(ProcessTerminationError::Kill(
                process_tree_source,
                child_source,
            )) => error.with_cleanup_failures([
                CommandCleanupFailure::ProcessTreeTermination {
                    source: process_tree_source,
                },
                CommandCleanupFailure::ChildTermination {
                    source: child_source,
                },
            ]),
        };
        self.finish_without_status(error)
    }

    /// Cleans up inherited output pipes after timer failure after exit.
    ///
    /// # Parameters
    ///
    /// * `status` - Exit status already reported for the direct child.
    /// * `source` - Timer or monotonic-clock failure to preserve.
    ///
    /// # Returns
    ///
    /// The preserved time error with helper cleanup guarantees.
    fn handle_time_error_after_exit(
        self,
        source: TimeError,
    ) -> Result<FinishedCommand, CommandError> {
        let error = CommandError::from_reason(
            self.command_text.clone(),
            CommandErrorReason::TimeFailed { source },
            None,
        );
        let error = self.finish_without_status(error);
        Err(error)
    }

    /// Best-effort helper cleanup after wait failures.
    ///
    /// # Parameters
    ///
    /// # Returns
    ///
    /// Preserved process-control error with complete I/O cleanup.
    #[must_use]
    fn collect_after_wait_error(mut self, error: CommandError) -> CommandError {
        let error = match self.terminate_child() {
            Ok(outcome) => {
                error.with_cleanup_failures(outcome.cleanup_failures)
            }
            Err(ProcessTerminationError::Wait(source)) => error
                .with_cleanup_failures([CommandCleanupFailure::Wait {
                    source,
                }]),
            Err(ProcessTerminationError::WaitAfterTreeTermination {
                wait_source,
                process_tree_source,
            }) => error.with_cleanup_failures([
                CommandCleanupFailure::Wait {
                    source: wait_source,
                },
                CommandCleanupFailure::ProcessTreeTermination {
                    source: process_tree_source,
                },
            ]),
            Err(ProcessTerminationError::Kill(
                process_tree_source,
                child_source,
            )) => error.with_cleanup_failures([
                CommandCleanupFailure::ProcessTreeTermination {
                    source: process_tree_source,
                },
                CommandCleanupFailure::ChildTermination {
                    source: child_source,
                },
            ]),
        };
        self.finish_without_status(error)
    }

    /// Best-effort helper cleanup when timeout/cancellation cleanup lost
    /// status.
    ///
    /// # Returns
    ///
    /// Preserved error after all helper joins attempt.
    #[must_use]
    fn collect_after_status_lost(self, error: CommandError) -> CommandError {
        self.finish_without_status(error)
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
