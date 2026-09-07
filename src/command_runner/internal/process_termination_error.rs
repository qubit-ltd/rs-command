// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal process-termination failure categories.

use std::io;

use super::stop_reason::StopReason;
use crate::CommandCleanupFailure;
use crate::CommandError;
use crate::CommandErrorReason;

/// Failure encountered while terminating and waiting for a running child.
#[derive(Debug)]
pub(super) enum ProcessTerminationError {
    /// Process-tree termination failed while the child was still running or
    /// its final status could not be confirmed.
    Kill(
        /// Operating-system process-tree termination error.
        io::Error,
        /// Fallback direct-child termination error.
        io::Error,
    ),
    /// Waiting for the child failed after termination was requested.
    Wait(
        /// Operating-system child wait error.
        io::Error,
    ),
    /// Waiting failed after process-tree termination had already failed.
    WaitAfterTreeTermination {
        /// Operating-system wait error.
        wait_source: io::Error,
        /// Earlier process-tree termination error.
        process_tree_source: io::Error,
    },
}

impl ProcessTerminationError {
    /// Maps a termination failure while preserving the initiating stop reason.
    ///
    /// Wait and time failures remain primary and receive termination failures
    /// as cleanup details. Timeout and cancellation use their dedicated kill
    /// failure reasons when both tree and direct-child termination fail.
    ///
    /// # Parameters
    ///
    /// * `stop_reason` - Reason that initiated process cleanup.
    /// * `command` - Human-readable command text for diagnostics.
    ///
    /// # Returns
    ///
    /// Structured command error preserving primary-error precedence.
    pub(super) fn into_command_error(self, stop_reason: StopReason, command: &str) -> CommandError {
        match (stop_reason, self) {
            (
                StopReason::TimedOut { timeout, .. },
                Self::Kill(process_tree_source, child_source),
            ) => CommandError::from_reason(
                command,
                CommandErrorReason::KillFailed {
                    timeout,
                    process_tree_source,
                    child_source,
                },
                None,
            ),
            (StopReason::Cancelled { .. }, Self::Kill(process_tree_source, child_source)) => {
                CommandError::from_reason(
                    command,
                    CommandErrorReason::CancelFailed {
                        process_tree_source,
                        child_source,
                    },
                    None,
                )
            }
            (reason @ (StopReason::WaitFailed(_) | StopReason::TimeFailed { .. }), failure) => {
                reason
                    .into_primary_error(command, None)
                    .with_cleanup_failures(failure.into_cleanup_failures())
            }
            (_, Self::Wait(source)) => {
                CommandError::from_reason(command, CommandErrorReason::WaitFailed { source }, None)
            }
            (
                _,
                Self::WaitAfterTreeTermination {
                    wait_source,
                    process_tree_source,
                },
            ) => CommandError::from_reason(
                command,
                CommandErrorReason::WaitFailed {
                    source: wait_source,
                },
                None,
            )
            .with_cleanup_failures([CommandCleanupFailure::ProcessTreeTermination {
                source: process_tree_source,
            }]),
        }
    }

    /// Converts termination failures into cleanup details.
    fn into_cleanup_failures(self) -> Vec<CommandCleanupFailure> {
        match self {
            Self::Wait(source) => vec![CommandCleanupFailure::Wait { source }],
            Self::WaitAfterTreeTermination {
                wait_source,
                process_tree_source,
            } => vec![
                CommandCleanupFailure::Wait {
                    source: wait_source,
                },
                CommandCleanupFailure::ProcessTreeTermination {
                    source: process_tree_source,
                },
            ],
            Self::Kill(process_tree_source, child_source) => vec![
                CommandCleanupFailure::ProcessTreeTermination {
                    source: process_tree_source,
                },
                CommandCleanupFailure::ChildTermination {
                    source: child_source,
                },
            ],
        }
    }
}
