// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
use std::io;
use std::process::ExitStatus;
#[cfg(coverage)]
use std::sync::atomic::AtomicBool;
#[cfg(coverage)]
use std::sync::atomic::Ordering;

use process_wrap::std::ChildWrapper;

#[cfg(coverage)]
static FAIL_TREE_KILL: AtomicBool = AtomicBool::new(false);

/// Enables deterministic process-tree termination failure injection.
#[cfg(coverage)]
pub(in crate::command_runner) fn __coverage_fail_tree_kill(enabled: bool) {
    FAIL_TREE_KILL.store(enabled, Ordering::Relaxed);
}

/// Child process wrapper with explicit process-tree capability tracking.
///
/// The `process_tree_managed` flag records whether the wrapped process is using
/// a process-group or job-object wrapper and therefore can be terminated as a
/// tree.
pub(in crate::command_runner) struct ManagedChildProcess {
    /// Wrapped process used for wait/try_wait operations.
    child: Box<dyn ChildWrapper>,
    /// Whether a process-tree management wrapper is currently active.
    process_tree_managed: bool,
}

impl ManagedChildProcess {
    /// Creates a managed child process wrapper.
    #[inline]
    pub(in crate::command_runner) fn new(
        child: Box<dyn ChildWrapper>,
        process_tree_managed: bool,
    ) -> Self {
        Self {
            child,
            process_tree_managed,
        }
    }

    /// Returns whether this child is wrapped for process-tree termination.
    #[inline]
    #[must_use]
    pub(in crate::command_runner) const fn process_tree_managed(&self) -> bool {
        self.process_tree_managed
    }

    /// Returns mutable access to the wrapped child handle.
    #[inline]
    pub(in crate::command_runner) fn wrapper_mut(
        &mut self,
    ) -> &mut dyn ChildWrapper {
        self.child.as_mut()
    }

    /// Attempts process-tree termination through the outer wrapper.
    ///
    /// This preserves wrapper semantics for process-group and job-object based
    /// descendant cleanup.
    #[inline]
    pub(in crate::command_runner) fn start_kill_tree(
        &mut self,
    ) -> io::Result<()> {
        #[cfg(coverage)]
        if FAIL_TREE_KILL.load(Ordering::Relaxed) {
            return Err(io::Error::other(
                "coverage-injected process-tree termination failure",
            ));
        }
        self.child.start_kill()
    }

    /// Attempts direct-child termination by bypassing process-wrap management.
    ///
    /// `inner_mut().start_kill()` intentionally targets the direct child
    /// process after a process-tree termination failure.
    #[inline]
    pub(in crate::command_runner) fn start_kill_child(
        &mut self,
    ) -> io::Result<()> {
        self.child.inner_mut().start_kill()
    }

    /// Checks non-blockingly whether the child has exited.
    #[inline]
    pub(in crate::command_runner) fn try_wait(
        &mut self,
    ) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Blocks until the child exits and returns its status.
    #[inline]
    pub(in crate::command_runner) fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }
}
