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
    io,
    process::{
        ChildStderr,
        ChildStdin,
        ChildStdout,
        ExitStatus,
    },
};

use process_wrap::std::ChildWrapper;

use super::success_status;

/// Child wrapper without a stdin pipe.
#[derive(Debug, Default)]
pub(super) struct NoStdinChild {
    /// Synthetic stdin pipe.
    pub(super) stdin: Option<ChildStdin>,
    /// Synthetic stdout pipe.
    pub(super) stdout: Option<ChildStdout>,
    /// Synthetic stderr pipe.
    pub(super) stderr: Option<ChildStderr>,
    /// Error returned by synthetic try-wait.
    pub(super) try_wait_error: Option<&'static str>,
    /// Whether the synthetic try-wait error is reported only once.
    pub(super) clear_try_wait_error_after_first: bool,
    /// Whether synthetic try-wait reports a still-running child.
    pub(super) pending: bool,
    /// Whether try-wait reports exit after a kill attempt.
    pub(super) exited_after_kill_attempt: bool,
    /// Whether kill has been attempted.
    pub(super) kill_attempted: bool,
    /// Error returned by synthetic kill.
    pub(super) kill_error: Option<&'static str>,
    /// Error returned by synthetic wait.
    pub(super) wait_error: Option<&'static str>,
}

impl ChildWrapper for NoStdinChild {
    /// Returns this synthetic child as the innermost wrapper.
    fn inner(&self) -> &dyn ChildWrapper {
        self
    }

    /// Returns this synthetic child as the innermost mutable wrapper.
    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        self
    }

    /// Consumes and returns this synthetic child.
    fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
        self
    }

    /// Returns the absent synthetic stdin pipe.
    fn stdin(&mut self) -> &mut Option<ChildStdin> {
        &mut self.stdin
    }

    /// Returns the absent synthetic stdout pipe.
    fn stdout(&mut self) -> &mut Option<ChildStdout> {
        &mut self.stdout
    }

    /// Returns the absent synthetic stderr pipe.
    fn stderr(&mut self) -> &mut Option<ChildStderr> {
        &mut self.stderr
    }

    /// Returns a dummy process identifier.
    fn id(&self) -> u32 {
        0
    }

    /// Reports that the synthetic child has already exited successfully.
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(message) = self.try_wait_error {
            if self.clear_try_wait_error_after_first {
                self.try_wait_error = None;
            }
            Err(io::Error::other(message))
        } else if self.pending && !(self.kill_attempted && self.exited_after_kill_attempt) {
            Ok(None)
        } else {
            Ok(Some(success_status()))
        }
    }

    /// Reports a successful synthetic process exit.
    fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(message) = self.wait_error {
            Err(io::Error::other(message))
        } else {
            Ok(success_status())
        }
    }

    /// Reports a successful synthetic kill.
    fn start_kill(&mut self) -> io::Result<()> {
        self.kill_attempted = true;
        if let Some(message) = self.kill_error {
            Err(io::Error::other(message))
        } else {
            Ok(())
        }
    }
}
