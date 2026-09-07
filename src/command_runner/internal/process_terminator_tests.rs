// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unit tests for process termination ordering and error mapping.

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

use super::managed_child_process::ManagedChildProcess;
use super::process_termination_error::ProcessTerminationError;
use super::process_terminator::ProcessTerminator;
use super::stop_reason::StopReason;
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
    fn new(
        name: &'static str,
        inner: Box<dyn ChildWrapper>,
        calls: Arc<Mutex<Vec<String>>>,
    ) -> Self {
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

    fn try_wait_results(
        mut self,
        results: impl IntoIterator<Item = io::Result<Option<ExitStatus>>>,
    ) -> Self {
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
fn test_process_termination_maps_timeout_and_cancellation_kill_failures() {
    let cases = [
        (
            StopReason::TimedOut {
                timeout: Duration::from_secs(2),
                status: None,
            },
            CommandErrorKind::KillFailed,
        ),
        (
            StopReason::Cancelled { status: None },
            CommandErrorKind::CancelFailed,
        ),
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
