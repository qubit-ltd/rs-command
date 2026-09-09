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

#[derive(Debug)]
pub(super) struct ScriptedChild {
    name: &'static str,
    inner: Box<dyn ChildWrapper>,
    calls: Arc<Mutex<Vec<String>>>,
    kill_error: Option<io::Error>,
    wait_result: Option<io::Result<ExitStatus>>,
    try_wait_results: VecDeque<io::Result<Option<ExitStatus>>>,
}

impl ScriptedChild {
    pub(super) fn new(name: &'static str, inner: Box<dyn ChildWrapper>, calls: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            name,
            inner,
            calls,
            kill_error: None,
            wait_result: None,
            try_wait_results: VecDeque::new(),
        }
    }

    pub(super) fn kill_error(mut self, source: io::Error) -> Self {
        self.kill_error = Some(source);
        self
    }

    pub(super) fn wait_status(mut self, exit_status: ExitStatus) -> Self {
        self.wait_result = Some(Ok(exit_status));
        self
    }

    pub(super) fn try_wait_results(
        mut self,
        results: impl IntoIterator<Item = io::Result<Option<ExitStatus>>>,
    ) -> Self {
        self.try_wait_results.extend(results);
        self
    }

    /// Injects a failure while confirming an accepted termination request.
    pub(super) fn wait_error(mut self, source: io::Error) -> Self {
        self.wait_result = Some(Err(source));
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
