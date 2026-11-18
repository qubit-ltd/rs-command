// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scripted process wrappers for lifecycle fault injection.
use std::collections::VecDeque;
use std::io;
use std::process::Command as ProcessCommand;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::Mutex;

use process_wrap::std::ChildWrapper;

/// Wraps a child process with scripted lifecycle results for fault-injection
/// tests.
#[must_use]
#[derive(Debug)]
pub(super) struct ScriptedChild {
    /// Label included in each recorded lifecycle operation.
    name: &'static str,
    /// Child wrapper that handles operations without an injected result.
    inner: Box<dyn ChildWrapper>,
    /// Shared, ordered log of lifecycle operations observed by the test.
    calls: Arc<Mutex<Vec<String>>>,
    /// Queued errors returned by successive termination requests.
    kill_errors: VecDeque<io::Error>,
    /// Whether unscripted termination should be delegated to the inner child.
    delegate_kill: bool,
    /// Queued results returned by successive blocking waits.
    wait_results: VecDeque<io::Result<ExitStatus>>,
    /// Queued results returned by successive nonblocking waits.
    try_wait_results: VecDeque<io::Result<Option<ExitStatus>>>,
}

impl ScriptedChild {
    /// Creates a wrapper that records operations and delegates unscripted work.
    pub(super) fn new(name: &'static str, inner: Box<dyn ChildWrapper>, calls: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            name,
            inner,
            calls,
            kill_errors: VecDeque::new(),
            delegate_kill: false,
            wait_results: VecDeque::new(),
            try_wait_results: VecDeque::new(),
        }
    }

    /// Queues an error for a termination request.
    pub(super) fn kill_error(mut self, source: io::Error) -> Self {
        self.kill_errors.push_back(source);
        self
    }

    pub(super) fn delegate_kill_to_inner(mut self) -> Self {
        self.delegate_kill = true;
        self
    }

    /// Queues a successful blocking wait with the supplied exit status.
    pub(super) fn wait_status(mut self, exit_status: ExitStatus) -> Self {
        self.wait_results.push_back(Ok(exit_status));
        self
    }

    /// Queues results for successive nonblocking wait calls.
    pub(super) fn try_wait_results(
        mut self,
        results: impl IntoIterator<Item = io::Result<Option<ExitStatus>>>,
    ) -> Self {
        self.try_wait_results.extend(results);
        self
    }

    /// Queues a failure while confirming an accepted termination request.
    pub(super) fn wait_error(mut self, source: io::Error) -> Self {
        self.wait_results.push_back(Err(source));
        self
    }

    /// Records a lifecycle operation using this child's label.
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
        match self.kill_errors.pop_front() {
            Some(source) => Err(source),
            None if self.delegate_kill => self.inner.start_kill(),
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
        self.wait_results.pop_front().unwrap_or_else(|| self.inner.wait())
    }
}

/// Returns an already-reaped child to keep fault-injection tests leak-free.
pub(super) fn raw_child() -> Box<dyn ChildWrapper> {
    let mut child = ProcessCommand::new("rustc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("test child should spawn");
    child.wait().expect("test child should be reaped");
    Box::new(child)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::process::Stdio;
    use std::sync::Arc;
    use std::sync::Mutex;

    use process_wrap::std::ChildWrapper;

    use super::ScriptedChild;

    #[test]
    fn test_inner_access_and_unwrap_preserve_real_child_lifecycle() {
        let child = Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("child should spawn");
        let expected_id = child.id();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut scripted = ScriptedChild::new("child", Box::new(child), Arc::clone(&calls));

        assert_eq!(scripted.inner().id(), expected_id);

        let status = scripted
            .inner_mut()
            .wait()
            .expect("mutable inner access should retain child behavior");
        assert!(status.success());
        assert!(calls.lock().unwrap().is_empty());

        let status = Box::new(scripted)
            .into_inner()
            .wait()
            .expect("unwrapped child should remain waitable");
        assert!(status.success());
    }
}
