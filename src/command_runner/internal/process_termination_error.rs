// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal process-termination failure categories.

use std::io;

/// Failure encountered while terminating and waiting for a running child.
pub(super) enum ProcessTerminationError {
    /// Process-tree termination failed while the child was still running or
    /// its final status could not be confirmed.
    Kill(
        /// Operating-system process-tree termination error.
        io::Error,
    ),
    /// Waiting for the child failed after termination was requested.
    Wait(
        /// Operating-system child wait error.
        io::Error,
    ),
}
