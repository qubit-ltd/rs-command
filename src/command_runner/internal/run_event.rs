// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Events observed while monitoring a running command.

use std::io;
use std::process::ExitStatus;
use std::time::Duration;

use qubit_clock::TimeError;

use super::stop_reason::StopReason;

/// Terminal event produced by the command monitoring loop.
pub(super) enum RunEvent {
    /// The direct child exited normally.
    Exited(ExitStatus),
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

impl RunEvent {
    /// Extracts a normal child exit status.
    ///
    /// # Returns
    ///
    /// The exit status for [`Self::Exited`], or the corresponding stop reason
    /// for every event that requires cleanup.
    pub(super) fn into_exit_status(self) -> Result<ExitStatus, StopReason> {
        match self {
            Self::Exited(status) => Ok(status),
            Self::TimedOut { timeout, status } => Err(StopReason::TimedOut { timeout, status }),
            Self::Cancelled { status } => Err(StopReason::Cancelled { status }),
            Self::WaitFailed(source) => Err(StopReason::WaitFailed(source)),
            Self::TimeFailed { source, status } => Err(StopReason::TimeFailed { source, status }),
        }
    }

}
