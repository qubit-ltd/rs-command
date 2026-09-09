// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Failures observed while terminating a process, independent of its stop
//! reason.

use super::stop_reason::StopReason;
use crate::CommandCleanupFailure;
use crate::CommandError;

/// Every process-control failure retained when final status is unavailable.
#[derive(Debug)]
pub(super) struct ProcessTerminationError {
    /// Failures in observation order, normalized when attached to CommandError.
    pub(super) cleanup_failures: Vec<CommandCleanupFailure>,
}

impl ProcessTerminationError {
    /// Attaches termination evidence without replacing the initiating reason.
    pub(super) fn into_command_error(self, reason: StopReason, command: &str) -> CommandError {
        reason
            .into_primary_error(command, None)
            .with_cleanup_failures(self.into_cleanup_failures())
    }

    /// Moves all observed process-control failures into the caller's cleanup
    /// report.
    pub(super) fn into_cleanup_failures(self) -> Vec<CommandCleanupFailure> {
        self.cleanup_failures
    }
}
