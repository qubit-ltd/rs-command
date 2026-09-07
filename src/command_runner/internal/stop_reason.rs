// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Reasons for stopping a running command and its I/O helpers.

use std::io;
use std::process::ExitStatus;
use std::time::Duration;

use qubit_clock::TimeError;

use crate::CommandError;
use crate::CommandErrorReason;
use crate::CommandOutput;

/// Primary reason that a running command must be stopped.
#[derive(Debug)]
pub(super) enum StopReason {
    /// The configured timeout elapsed.
    TimedOut {
        /// Timeout exceeded by the command.
        timeout: Duration,
        /// Direct-child status already observed while collecting output.
        status: Option<ExitStatus>,
    },
    /// Cancellation was requested.
    Cancelled {
        /// Direct-child status already observed while collecting output.
        status: Option<ExitStatus>,
    },
    /// Waiting for the direct child failed.
    WaitFailed(io::Error),
    /// Monotonic time handling failed.
    TimeFailed {
        /// Timer or monotonic-clock failure.
        source: TimeError,
        /// Direct-child status already observed while collecting output.
        status: Option<ExitStatus>,
    },
}

impl StopReason {
    /// Returns a direct-child status observed before cleanup began.
    ///
    /// # Returns
    ///
    /// The observed status for post-exit failures, otherwise `None`.
    #[must_use]
    pub(super) const fn observed_status(&self) -> Option<ExitStatus> {
        match self {
            Self::TimedOut { status, .. }
            | Self::Cancelled { status }
            | Self::TimeFailed { status, .. } => *status,
            Self::WaitFailed(_) => None,
        }
    }

    /// Reports whether successful termination should produce captured output.
    ///
    /// Timeout and cancellation always report the final termination status.
    /// Wait and time failures do so only when monitoring had already observed
    /// the direct child exit.
    #[must_use]
    pub(super) const fn retains_termination_output(&self) -> bool {
        match self {
            Self::TimedOut { .. } | Self::Cancelled { .. } => true,
            Self::WaitFailed(_) => false,
            Self::TimeFailed { status, .. } => status.is_some(),
        }
    }

    /// Builds the primary command error represented by this reason.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `output` - Captured output retained while stopping the command.
    ///
    /// # Returns
    ///
    /// Structured command error preserving this primary reason and output.
    pub(super) fn into_primary_error(
        self,
        command: impl Into<String>,
        output: Option<Box<CommandOutput>>,
    ) -> CommandError {
        let reason = match self {
            Self::TimedOut { timeout, .. } => CommandErrorReason::TimedOut { timeout },
            Self::Cancelled { .. } => CommandErrorReason::Cancelled,
            Self::WaitFailed(source) => CommandErrorReason::WaitFailed { source },
            Self::TimeFailed { source, .. } => CommandErrorReason::TimeFailed { source },
        };
        CommandError::from_reason(command, reason, output)
    }
}
