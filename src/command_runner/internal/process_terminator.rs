// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Managed process-tree termination with direct-child fallback.

use std::io;
use std::process::ExitStatus;
use std::thread;
use std::time::Duration;

use super::managed_child_process::ManagedChildProcess;
use super::process_termination_error::ProcessTerminationError;
use super::process_termination_outcome::ProcessTerminationOutcome;
use crate::CommandCleanupFailure;

/// Bounded confirmation attempts for a child racing tree termination.
const KILL_FAILURE_EXIT_CHECK_ATTEMPTS: usize = 8;
/// Delay between bounded exit checks after a failed tree termination.
const KILL_FAILURE_EXIT_CHECK_DELAY: Duration = Duration::from_micros(50);

/// Terminates one managed child process and its process tree.
pub(super) struct ProcessTerminator<'a> {
    /// Managed child whose process tree must be stopped.
    child: &'a mut ManagedChildProcess,
}

impl<'a> ProcessTerminator<'a> {
    /// Creates a process terminator for a managed child.
    ///
    /// # Parameters
    ///
    /// * `child` - Managed child whose process tree must be stopped.
    ///
    /// # Returns
    ///
    /// A terminator borrowing the child for the cleanup operation.
    pub(super) const fn new(child: &'a mut ManagedChildProcess) -> Self {
        Self { child }
    }

    /// Terminates the managed process tree and returns final child status.
    ///
    /// Process-tree managed children are stopped through the outer wrapper
    /// first. A failed tree termination is followed by bounded status checks
    /// and then direct-child fallback. An already observed status is retained
    /// without waiting for the direct child a second time.
    ///
    /// # Parameters
    ///
    /// * `observed_status` - Direct-child status observed before cleanup.
    ///
    /// # Returns
    ///
    /// Final child status and non-fatal cleanup failures.
    ///
    /// # Errors
    ///
    /// Returns process termination or wait failures when no final status can
    /// be established.
    pub(super) fn terminate(
        mut self,
        observed_status: Option<ExitStatus>,
    ) -> Result<ProcessTerminationOutcome, ProcessTerminationError> {
        if !self.child.process_tree_managed() {
            if let Some(status) = observed_status {
                return Ok(Self::success(status));
            }
            if let Err(child_source) = self.child.start_kill_child() {
                let status = self
                    .child
                    .try_wait()
                    .map_err(ProcessTerminationError::Wait)?;
                if let Some(status) = status {
                    return Ok(Self::success(status));
                }
                return Err(ProcessTerminationError::Kill(
                    io::Error::other("direct kill used without tree management"),
                    child_source,
                ));
            }
            return self
                .child
                .wait()
                .map(Self::success)
                .map_err(ProcessTerminationError::Wait);
        }

        match self.child.start_kill_tree() {
            Ok(()) => match observed_status {
                Some(status) => Ok(Self::success(status)),
                None => self
                    .child
                    .wait()
                    .map(Self::success)
                    .map_err(ProcessTerminationError::Wait),
            },
            Err(process_tree_source) => {
                if let Some(status) = observed_status {
                    return Ok(Self::tree_failure_outcome(status, process_tree_source));
                }
                match self.status_after_termination_failure(&process_tree_source) {
                    Ok(Some(status)) => Ok(Self::tree_failure_outcome(status, process_tree_source)),
                    Ok(None) => match self.child.start_kill_child() {
                        Ok(()) => match self.child.wait() {
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
                        },
                        Err(child_source) => {
                            let status = self
                                .child
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
                    Err(wait_source) => Err(ProcessTerminationError::WaitAfterTreeTermination {
                        wait_source,
                        process_tree_source,
                    }),
                }
            }
        }
    }

    /// Builds a successful outcome without cleanup failures.
    fn success(status: ExitStatus) -> ProcessTerminationOutcome {
        ProcessTerminationOutcome {
            status,
            cleanup_failures: Vec::new(),
        }
    }

    /// Builds an outcome after a process-tree termination failure.
    fn tree_failure_outcome(
        status: ExitStatus,
        process_tree_source: io::Error,
    ) -> ProcessTerminationOutcome {
        let cleanup_failures = if Self::process_tree_already_exited(&process_tree_source) {
            Vec::new()
        } else {
            vec![CommandCleanupFailure::ProcessTreeTermination {
                source: process_tree_source,
            }]
        };
        ProcessTerminationOutcome {
            status,
            cleanup_failures,
        }
    }

    /// Resolves child status after process-tree termination failure.
    fn status_after_termination_failure(
        &mut self,
        source: &io::Error,
    ) -> io::Result<Option<ExitStatus>> {
        if Self::process_tree_already_exited(source) {
            return self.child.wait().map(Some);
        }
        for attempt in 0..KILL_FAILURE_EXIT_CHECK_ATTEMPTS {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Some(status));
            }
            if attempt + 1 < KILL_FAILURE_EXIT_CHECK_ATTEMPTS {
                thread::sleep(KILL_FAILURE_EXIT_CHECK_DELAY);
            }
        }
        Ok(None)
    }

    /// Reports whether the platform says the managed tree no longer exists.
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
}
