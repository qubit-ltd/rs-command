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
        let mut failures = Vec::new();
        if self.child.process_tree_managed() {
            match self.child.start_kill_tree() {
                Ok(()) => return self.finish_wait(observed_status, failures),
                Err(source) => {
                    let tree_gone = Self::process_tree_already_exited(&source);
                    if let Some(status) = observed_status {
                        if !tree_gone {
                            failures.push(CommandCleanupFailure::ProcessTreeTermination { source });
                        }
                        return Ok(Self::success(status, failures));
                    }
                    failures.push(CommandCleanupFailure::ProcessTreeTermination { source });
                    for attempt in 0..KILL_FAILURE_EXIT_CHECK_ATTEMPTS {
                        match self.child.try_wait() {
                            Ok(Some(status)) => {
                                if tree_gone {
                                    let _ = failures.remove(0);
                                }
                                return Ok(Self::success(status, failures));
                            }
                            Ok(None) => {}
                            Err(source) => {
                                failures.push(CommandCleanupFailure::Wait { source });
                                break;
                            }
                        }
                        if attempt + 1 < KILL_FAILURE_EXIT_CHECK_ATTEMPTS {
                            thread::sleep(KILL_FAILURE_EXIT_CHECK_DELAY);
                        }
                    }
                }
            }
        } else if let Some(status) = observed_status {
            return Ok(Self::success(status, failures));
        }

        match self.child.start_kill_child() {
            Ok(()) => self.finish_wait(None, failures),
            Err(source) => {
                failures.push(CommandCleanupFailure::ChildTermination { source });
                match self.child.try_wait() {
                    Ok(Some(status)) => Ok(Self::success(status, failures)),
                    Ok(None) => Err(ProcessTerminationError {
                        cleanup_failures: failures,
                    }),
                    Err(source) => {
                        failures.push(CommandCleanupFailure::Wait { source });
                        Err(ProcessTerminationError {
                            cleanup_failures: failures,
                        })
                    }
                }
            }
        }
    }

    /// Reaps only after a successful termination request, preserving prior
    /// failures.
    ///
    /// An observed status avoids waiting again. OS wait errors retain all
    /// earlier failures; successful termination requests do not imply a
    /// hard deadline.
    fn finish_wait(
        &mut self,
        status: Option<ExitStatus>,
        mut failures: Vec<CommandCleanupFailure>,
    ) -> Result<ProcessTerminationOutcome, ProcessTerminationError> {
        let result = match status {
            Some(status) => Ok(status),
            None => self.child.wait(),
        };
        match result {
            Ok(status) => Ok(Self::success(status, failures)),
            Err(source) => {
                failures.push(CommandCleanupFailure::Wait { source });
                Err(ProcessTerminationError {
                    cleanup_failures: failures,
                })
            }
        }
    }

    /// Builds an outcome retaining every preceding termination failure.
    fn success(status: ExitStatus, cleanup_failures: Vec<CommandCleanupFailure>) -> ProcessTerminationOutcome {
        ProcessTerminationOutcome {
            status,
            cleanup_failures,
        }
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

#[cfg(test)]
mod tests {
    use std::io;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::process::Command as ProcessCommand;
    use std::process::ExitStatus;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;

    use super::super::managed_child_process::ManagedChildProcess;
    use super::super::process_termination_error::ProcessTerminationError;
    use super::super::scripted_child::ScriptedChild;
    use super::super::scripted_child::raw_child;
    use super::super::stop_reason::StopReason;
    use super::ProcessTerminator;
    use crate::CommandCleanupFailure;
    use crate::CommandErrorKind;

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code as u32)
    }

    fn process_tree_not_found() -> io::Error {
        #[cfg(unix)]
        {
            io::Error::from_raw_os_error(libc::ESRCH)
        }
        #[cfg(not(unix))]
        {
            io::Error::from(io::ErrorKind::NotFound)
        }
    }

    #[test]
    fn test_process_terminator_treats_not_found_tree_as_exit_race() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let direct = ScriptedChild::new("child", raw_child(), Arc::clone(&calls));
        let tree = ScriptedChild::new("tree", Box::new(direct), Arc::clone(&calls))
            .kill_error(process_tree_not_found())
            .try_wait_results([Ok(Some(status(17)))]);
        let mut child = ManagedChildProcess::new(Box::new(tree), true);

        let outcome = ProcessTerminator::new(&mut child)
            .terminate(None)
            .expect("not-found race should retain the final status");

        assert_eq!(outcome.status.code(), Some(17));
        assert!(outcome.cleanup_failures.is_empty());
        assert_eq!(
            *calls.lock().expect("call log should not be poisoned"),
            ["tree.kill", "tree.try_wait"]
        );
    }

    #[test]
    fn test_process_terminator_falls_back_to_direct_child_after_tree_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let direct = ScriptedChild::new("child", raw_child(), Arc::clone(&calls));
        let tree = ScriptedChild::new("tree", Box::new(direct), Arc::clone(&calls))
            .kill_error(io::Error::other("tree kill failed"))
            .try_wait_results((0..8).map(|_| Ok(None)))
            .wait_status(status(18));
        let mut child = ManagedChildProcess::new(Box::new(tree), true);

        let outcome = ProcessTerminator::new(&mut child)
            .terminate(None)
            .expect("direct-child fallback should recover termination");

        assert_eq!(outcome.status.code(), Some(18));
        assert_eq!(outcome.cleanup_failures.len(), 1);
        let calls = calls.lock().expect("call log should not be poisoned");
        assert_eq!(calls.first().map(String::as_str), Some("tree.kill"));
        assert_eq!(calls.get(9).map(String::as_str), Some("child.kill"));
        assert_eq!(calls.last().map(String::as_str), Some("tree.wait"));
    }

    #[test]
    fn test_process_terminator_waits_after_successful_tree_termination() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let tree = ScriptedChild::new("tree", raw_child(), Arc::clone(&calls));
        let mut child = ManagedChildProcess::new(Box::new(tree), true);

        let outcome = ProcessTerminator::new(&mut child)
            .terminate(None)
            .expect("successful tree termination should wait for final status");

        assert!(outcome.status.success());
        assert!(outcome.cleanup_failures.is_empty());
        assert_eq!(
            *calls.lock().expect("call log should not be poisoned"),
            ["tree.kill", "tree.wait"]
        );
    }

    #[test]
    fn test_process_terminator_observes_completed_child_after_tree_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut completed = ProcessCommand::new("rustc")
            .arg("--version")
            .spawn()
            .expect("test child should spawn");
        let expected_status = completed.wait().expect("test child should exit");
        let tree = ScriptedChild::new("tree", Box::new(completed), Arc::clone(&calls))
            .kill_error(io::Error::other("tree kill failed"));
        let mut child = ManagedChildProcess::new(Box::new(tree), true);

        let outcome = ProcessTerminator::new(&mut child)
            .terminate(None)
            .expect("completed child status should resolve tree-kill failure");

        assert_eq!(outcome.status, expected_status);
        assert_eq!(outcome.cleanup_failures.len(), 1);
        assert_eq!(
            *calls.lock().expect("call log should not be poisoned"),
            ["tree.kill", "tree.try_wait"]
        );
    }

    #[test]
    fn test_process_termination_maps_timeout_and_cancellation_kill_failures() {
        let cases = [
            (
                StopReason::TimedOut {
                    timeout: Duration::from_secs(2),
                    status: None,
                },
                CommandErrorKind::TimedOut,
            ),
            (StopReason::Cancelled { status: None }, CommandErrorKind::Cancelled),
        ];

        for (reason, expected_kind) in cases {
            let error = ProcessTerminationError {
                cleanup_failures: vec![
                    CommandCleanupFailure::ProcessTreeTermination {
                        source: io::Error::other("tree kill failed"),
                    },
                    CommandCleanupFailure::ChildTermination {
                        source: io::Error::other("child kill failed"),
                    },
                ],
            }
            .into_command_error(reason, "command");
            assert_eq!(error.kind(), expected_kind);
        }
    }
    #[test]
    fn test_process_terminator_retains_every_failure_without_waiting_after_failed_kills() {
        for wait_error in [false, true] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let direct = ScriptedChild::new("child", raw_child(), Arc::clone(&calls))
                .kill_error(io::Error::other("child denied"));
            let last = if wait_error {
                Err(io::Error::other("status failed"))
            } else {
                Ok(None)
            };
            let tree = ScriptedChild::new("tree", Box::new(direct), Arc::clone(&calls))
                .kill_error(io::Error::other("tree denied"))
                .try_wait_results((0..8).map(|_| Ok(None)).chain(std::iter::once(last)));
            let mut child = ManagedChildProcess::new(Box::new(tree), true);
            let error = ProcessTerminator::new(&mut child)
                .terminate(None)
                .expect_err("both rejected kills must fail");
            assert_eq!(error.cleanup_failures.len(), if wait_error { 3 } else { 2 });
            assert!(matches!(
                error.cleanup_failures[0],
                CommandCleanupFailure::ProcessTreeTermination { .. }
            ));
            assert!(matches!(
                error.cleanup_failures[1],
                CommandCleanupFailure::ChildTermination { .. }
            ));
            if wait_error {
                assert!(matches!(error.cleanup_failures[2], CommandCleanupFailure::Wait { .. }));
            }
            let calls = calls.lock().expect("calls should be readable");
            assert!(!calls.iter().any(|call| call.ends_with(".wait")));
            assert_eq!(calls.iter().filter(|call| call.as_str() == "child.kill").count(), 1);
        }
    }

    #[test]
    fn test_process_terminator_keeps_tree_failure_when_fallback_wait_fails() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let direct = ScriptedChild::new("child", raw_child(), Arc::clone(&calls));
        let tree = ScriptedChild::new("tree", Box::new(direct), Arc::clone(&calls))
            .kill_error(io::Error::other("tree denied"))
            .try_wait_results([Err(io::Error::other("initial status failed"))])
            .wait_error(io::Error::other("final status failed"));
        let mut child = ManagedChildProcess::new(Box::new(tree), true);
        let error = ProcessTerminator::new(&mut child)
            .terminate(None)
            .expect_err("wait failure must be retained");
        assert!(matches!(
            error.cleanup_failures.as_slice(),
            [
                CommandCleanupFailure::ProcessTreeTermination { .. },
                CommandCleanupFailure::Wait { .. },
                CommandCleanupFailure::Wait { .. }
            ]
        ));
        assert!(
            calls
                .lock()
                .expect("calls should be readable")
                .contains(&"child.kill".to_owned())
        );
    }

    #[test]
    fn test_process_terminator_does_not_wait_for_observed_status() {
        for managed in [false, true] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let tree =
                ScriptedChild::new("tree", raw_child(), Arc::clone(&calls)).kill_error(io::Error::other("tree denied"));
            let mut child = ManagedChildProcess::new(Box::new(tree), managed);
            let result = ProcessTerminator::new(&mut child)
                .terminate(Some(status(9)))
                .expect("observed status must survive");
            assert_eq!(result.status.code(), Some(9));
            assert_eq!(result.cleanup_failures.len(), usize::from(managed));
            assert!(
                !calls
                    .lock()
                    .expect("calls should be readable")
                    .iter()
                    .any(|call| call.ends_with("wait"))
            );
        }
    }
}
