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
                let status = self.child.try_wait().map_err(ProcessTerminationError::Wait)?;
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
                                cleanup_failures: vec![CommandCleanupFailure::ProcessTreeTermination {
                                    source: process_tree_source,
                                }],
                            }),
                            Err(wait_source) => Err(ProcessTerminationError::WaitAfterTreeTermination {
                                wait_source,
                                process_tree_source,
                            }),
                        },
                        Err(child_source) => {
                            let status = self.child.try_wait().map_err(ProcessTerminationError::Wait)?;
                            if let Some(status) = status {
                                Ok(ProcessTerminationOutcome {
                                    status,
                                    cleanup_failures: vec![
                                        CommandCleanupFailure::ProcessTreeTermination {
                                            source: process_tree_source,
                                        },
                                        CommandCleanupFailure::ChildTermination { source: child_source },
                                    ],
                                })
                            } else {
                                Err(ProcessTerminationError::Kill(process_tree_source, child_source))
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
    fn tree_failure_outcome(status: ExitStatus, process_tree_source: io::Error) -> ProcessTerminationOutcome {
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
    fn status_after_termination_failure(&mut self, source: &io::Error) -> io::Result<Option<ExitStatus>> {
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
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

    use process_wrap::std::ChildWrapper;

    use super::super::managed_child_process::ManagedChildProcess;
    use super::super::process_termination_error::ProcessTerminationError;
    use super::super::stop_reason::StopReason;
    use super::ProcessTerminator;
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

    #[derive(Debug)]
    struct ScriptedChild {
        name: &'static str,
        inner: Box<dyn ChildWrapper>,
        calls: Arc<Mutex<Vec<String>>>,
        kill_error: Option<io::Error>,
        wait_result: Option<io::Result<ExitStatus>>,
        try_wait_results: VecDeque<io::Result<Option<ExitStatus>>>,
    }

    impl ScriptedChild {
        fn new(name: &'static str, inner: Box<dyn ChildWrapper>, calls: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                name,
                inner,
                calls,
                kill_error: None,
                wait_result: None,
                try_wait_results: VecDeque::new(),
            }
        }

        fn kill_error(mut self, source: io::Error) -> Self {
            self.kill_error = Some(source);
            self
        }

        fn wait_status(mut self, exit_status: ExitStatus) -> Self {
            self.wait_result = Some(Ok(exit_status));
            self
        }

        fn try_wait_results(mut self, results: impl IntoIterator<Item = io::Result<Option<ExitStatus>>>) -> Self {
            self.try_wait_results.extend(results);
            self
        }

        fn record(&self, operation: &str) {
            self.calls
                .lock()
                .expect("call log should not be poisoned")
                .push(format!("{}.{}", self.name, operation));
        }
    }

    impl ChildWrapper for ScriptedChild {
        fn inner(&self) -> &dyn ChildWrapper {
            self.inner.as_ref()
        }

        fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
            self.inner.as_mut()
        }

        fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
            self.inner
        }

        fn start_kill(&mut self) -> io::Result<()> {
            self.record("kill");
            match self.kill_error.take() {
                Some(source) => Err(source),
                None => Ok(()),
            }
        }

        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            self.record("try_wait");
            self.try_wait_results
                .pop_front()
                .unwrap_or_else(|| self.inner.try_wait())
        }

        fn wait(&mut self) -> io::Result<ExitStatus> {
            self.record("wait");
            self.wait_result.take().unwrap_or_else(|| self.inner.wait())
        }
    }

    fn raw_child() -> Box<dyn ChildWrapper> {
        Box::new(
            ProcessCommand::new("rustc")
                .arg("--version")
                .spawn()
                .expect("test child should spawn"),
        )
    }

    #[test]
    fn test_process_terminator_treats_not_found_tree_as_exit_race() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let direct = ScriptedChild::new("child", raw_child(), Arc::clone(&calls));
        let tree = ScriptedChild::new("tree", Box::new(direct), Arc::clone(&calls))
            .kill_error(process_tree_not_found())
            .wait_status(status(17));
        let mut child = ManagedChildProcess::new(Box::new(tree), true);

        let outcome = ProcessTerminator::new(&mut child)
            .terminate(None)
            .expect("not-found race should retain the final status");

        assert_eq!(outcome.status.code(), Some(17));
        assert!(outcome.cleanup_failures.is_empty());
        assert_eq!(
            *calls.lock().expect("call log should not be poisoned"),
            ["tree.kill", "tree.wait"]
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
                CommandErrorKind::KillFailed,
            ),
            (StopReason::Cancelled { status: None }, CommandErrorKind::CancelFailed),
        ];

        for (reason, expected_kind) in cases {
            let error = ProcessTerminationError::Kill(
                io::Error::other("tree kill failed"),
                io::Error::other("child kill failed"),
            )
            .into_command_error(reason, "command");
            assert_eq!(error.kind(), expected_kind);
        }
    }
}
