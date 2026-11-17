// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::Mutex;

use qubit_operation::AsyncOperationHandle;

use crate::CommandError;
use crate::CommandOutput;

/// Asynchronous command operation handle with access to diagnostics retained
/// when a managed stop produces the upstream `Stopped` outcome.
#[must_use = "retain the handle to observe the command outcome and settlement"]
pub struct AsyncCommandOperation {
    /// The underlying asynchronous operation manager handle.
    inner: AsyncOperationHandle<CommandOutput, CommandError>,
    /// Diagnostic slot shared with the command backend.
    stop_diagnostic: Arc<Mutex<Option<CommandError>>>,
}

impl AsyncCommandOperation {
    /// Wraps the upstream asynchronous handle with command diagnostics.
    ///
    /// # Parameters
    /// * `inner`: The upstream async operation handle.
    /// * `stop_diagnostic`: Shared diagnostic slot owned by the command
    ///   adapter.
    pub(crate) fn new(
        inner: AsyncOperationHandle<CommandOutput, CommandError>,
        stop_diagnostic: Arc<Mutex<Option<CommandError>>>,
    ) -> Self {
        Self { inner, stop_diagnostic }
    }

    /// Takes partial output and cleanup details retained for a stopped result.
    ///
    /// Call after the operation settlement is available so backend cleanup has
    /// finished publishing the diagnostic.
    #[must_use]
    pub fn take_stop_diagnostic(&mut self) -> Option<CommandError> {
        self.stop_diagnostic
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    /// Moves this handle into its synchronous view.
    #[must_use = "retain the synchronous operation handle"]
    pub fn into_sync(self) -> crate::CommandOperation {
        crate::CommandOperation::new(self.inner.into_sync(), self.stop_diagnostic)
    }

    /// Returns the underlying asynchronous handle, discarding access to the
    /// command-specific stop diagnostic slot.
    #[must_use = "retain the underlying asynchronous operation handle"]
    pub fn into_inner(self) -> AsyncOperationHandle<CommandOutput, CommandError> {
        self.inner
    }
}

impl Deref for AsyncCommandOperation {
    /// The upstream asynchronous operation handle type.
    type Target = AsyncOperationHandle<CommandOutput, CommandError>;

    /// Borrows the upstream asynchronous operation handle.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for AsyncCommandOperation {
    /// Mutably borrows the upstream asynchronous operation handle.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
