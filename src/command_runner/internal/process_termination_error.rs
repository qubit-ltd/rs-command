// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow multiple-public-types
//! Internal process-termination failure categories.

use std::io;
use std::process::ExitStatus;

use crate::CommandCleanupFailure;

/// Successful process termination together with cleanup failures observed
/// while reaching the final child status.
pub(super) struct ProcessTerminationOutcome {
    /// Final status reported by the direct child.
    pub(super) status: ExitStatus,
    /// Non-fatal termination failures to retain beside the primary reason.
    pub(super) cleanup_failures: Vec<CommandCleanupFailure>,
}

/// Failure encountered while terminating and waiting for a running child.
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
