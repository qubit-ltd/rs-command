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
use crate::CommandErrorReason;

/// Every process-control failure retained when final status is unavailable.
#[derive(Debug)]
pub(super) struct ProcessTerminationError {
    /// Failures in observation order, normalized when attached to CommandError.
    pub(super) cleanup_failures: Vec<CommandCleanupFailure>,
}

impl ProcessTerminationError {
    /// Attaches termination evidence without replacing the initiating reason.
    ///
    /// # Parameters
    ///
    /// * `reason` - The event that caused process termination.
    /// * `command` - The redacted command description associated with the
    ///   event.
    ///
    /// # Returns
    ///
    /// A command error containing the initiating reason and all cleanup
    /// failures.
    pub(super) fn into_command_error(self, reason: StopReason, command: &str) -> CommandError {
        if matches!(reason, StopReason::OutputReadFailed { .. }) {
            let mut source = None;
            let mut cleanup = Vec::new();
            for failure in self.cleanup_failures {
                let failure = match failure {
                    CommandCleanupFailure::Wait { source: error }
                    | CommandCleanupFailure::ProcessTreeTermination { source: error }
                    | CommandCleanupFailure::ChildTermination { source: error }
                        if source.is_none() =>
                    {
                        source = Some(error);
                        continue;
                    }
                    other => other,
                };
                cleanup.push(failure);
            }
            let error = CommandError::from_reason(
                command,
                CommandErrorReason::WaitFailed {
                    source: source.unwrap_or_else(|| {
                        std::io::Error::other("process termination failed without recorded evidence")
                    }),
                },
                None,
            );
            return error.with_cleanup_failures(cleanup);
        }
        reason
            .into_primary_error(command, None)
            .with_cleanup_failures(self.into_cleanup_failures())
    }

    /// Moves all observed process-control failures into the caller's cleanup
    /// report.
    ///
    /// # Returns
    ///
    /// Cleanup failures in the order in which they were observed.
    #[must_use]
    #[inline]
    pub(super) fn into_cleanup_failures(self) -> Vec<CommandCleanupFailure> {
        self.cleanup_failures
    }
}

#[cfg(test)]
mod tests {
    use super::super::stop_reason::StopReason;
    use super::ProcessTerminationError;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;

    #[test]
    fn test_output_read_termination_failure_without_evidence_uses_internal_fallback() {
        let error = ProcessTerminationError {
            cleanup_failures: Vec::new(),
        }
        .into_command_error(StopReason::OutputReadFailed { status: None }, "command");

        assert_eq!(error.kind(), CommandErrorKind::WaitFailed);
        assert!(matches!(
            error.reason(),
            CommandErrorReason::WaitFailed { source }
                if source.to_string().contains("process termination failed without recorded evidence")
        ));
    }
}
