/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::{
    process::ExitStatus,
    thread,
    time::{
        Duration,
        Instant,
    },
};

use super::{
    command_io::CommandIo,
    error_mapping::{
        kill_failed,
        wait_failed,
    },
    finished_command::FinishedCommand,
    managed_child_process::ManagedChildProcess,
    wait_policy::next_sleep,
};
use crate::CommandError;

/// Running command state that owns process and I/O helper lifetimes.
pub(crate) struct RunningCommand {
    /// Human-readable command text for diagnostics.
    command_text: String,
    /// Child process managed by the command runner.
    child_process: ManagedChildProcess,
    /// Output readers and optional stdin writer.
    io: CommandIo,
    /// Time when the child process started being monitored.
    started_at: Instant,
}

impl RunningCommand {
    /// Creates a running command state object.
    ///
    /// # Parameters
    ///
    /// * `command_text` - Human-readable command text for diagnostics.
    /// * `child_process` - Child process managed by the runner.
    /// * `io` - Output readers and optional stdin writer.
    ///
    /// # Returns
    ///
    /// Running command state that owns the process and its I/O helpers.
    pub(crate) fn new(
        command_text: String,
        child_process: ManagedChildProcess,
        io: CommandIo,
    ) -> Self {
        Self {
            command_text,
            child_process,
            io,
            started_at: Instant::now(),
        }
    }

    /// Waits for the child process to complete or time out.
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
    /// Returns [`CommandError`] if waiting fails, timeout handling fails, output
    /// collection fails, or stdin writing fails. Wait-error cleanup only joins I/O
    /// helpers after a non-blocking check confirms the child has exited.
    pub(crate) fn wait_for_completion(
        mut self,
        timeout: Option<Duration>,
    ) -> Result<FinishedCommand, CommandError> {
        loop {
            let maybe_status = match self.child_process.try_wait() {
                Ok(status) => status,
                Err(source) => {
                    let error = wait_failed(&self.command_text, source);
                    return Err(self.clean_up_after_wait_error(error));
                }
            };
            if let Some(status) = maybe_status {
                return self.complete(status);
            }
            if let Some(timeout) = timeout
                && self.started_at.elapsed() >= timeout
            {
                return self.handle_timeout(timeout);
            }
            thread::sleep(next_sleep(timeout, self.started_at.elapsed()));
        }
    }

    /// Handles timeout by killing the child process and collecting final output.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Timeout that has been exceeded.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::TimedOut`] after successful kill and wait, or the
    /// process-control error if killing or waiting fails. Cleanup after those
    /// errors only joins I/O helpers if the child is already confirmed exited.
    fn handle_timeout(mut self, timeout: Duration) -> Result<FinishedCommand, CommandError> {
        if let Err(source) = self.child_process.start_kill() {
            let error = kill_failed(self.command_text.clone(), timeout, source);
            return Err(self.collect_if_child_exited(error));
        }
        let exit_status = match self.child_process.wait() {
            Ok(status) => status,
            Err(source) => {
                let error = wait_failed(&self.command_text, source);
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
    fn complete(self, status: ExitStatus) -> Result<FinishedCommand, CommandError> {
        let output = self
            .io
            .collect(&self.command_text, status, self.started_at.elapsed())?;
        Ok(FinishedCommand {
            command_text: self.command_text,
            output,
        })
    }

    /// Attempts non-blocking cleanup after a wait error.
    ///
    /// # Parameters
    ///
    /// * `error` - Original wait error to preserve.
    ///
    /// # Returns
    ///
    /// The original error after best-effort cleanup. This method deliberately does
    /// not call blocking wait APIs because it is already handling a wait failure.
    fn clean_up_after_wait_error(mut self, error: CommandError) -> CommandError {
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
    /// The original error. Output collection failures during cleanup are ignored
    /// so the primary process-control failure remains visible.
    fn collect_if_child_exited(mut self, error: CommandError) -> CommandError {
        if let Ok(Some(status)) = self.child_process.try_wait() {
            let _ = self.complete(status);
        }
        error
    }
}
