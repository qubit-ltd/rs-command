// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Represents the cleanup state or contract used by managed command execution.
type Cleanup = Box<dyn FnOnce() + Send + 'static>;
/// Represents the reaper owner state or contract used by managed command
/// execution.
pub(crate) struct ReaperOwner(#[doc = "Cleanup action retained until its owner is safe to release."] Option<Cleanup>);

impl std::fmt::Debug for ReaperOwner {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `formatter`: Supplies the formatter value used by this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ReaperOwner").finish_non_exhaustive()
    }
}

impl ReaperOwner {
    /// Stores the new value used by this execution component.
    ///
    /// # Parameters
    /// * `cleanup`: Supplies the cleanup value used by this operation.
    pub(crate) fn new(cleanup: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(cleanup)))
    }

    /// Performs the reap step in the managed command execution lifecycle.
    pub(in crate::execution::backend) fn reap(mut self) {
        if let Some(cleanup) = self.0.take() {
            cleanup();
        }
    }

    /// Performs the run inline step in the managed command execution lifecycle.
    pub(crate) fn run_inline(self) {
        self.reap();
    }
}
