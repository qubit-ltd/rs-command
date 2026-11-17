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

use qubit_operation::OperationHandle;

use crate::CommandError;
use crate::CommandOutput;

/// Synchronous command operation handle with access to diagnostics retained
/// when a managed stop produces the upstream `Stopped` outcome.
#[must_use = "retain the handle to observe the command outcome and settlement"]
pub struct CommandOperation {
    /// The underlying operation manager handle.
    inner: OperationHandle<CommandOutput, CommandError>,
    /// Diagnostic slot shared with the domain backend.
    stop_diagnostic: Arc<Mutex<Option<CommandError>>>,
}

impl CommandOperation {
    /// Wraps the upstream operation handle with command diagnostics.
    ///
    /// # Parameters
    /// * `inner`: The upstream operation handle.
    /// * `stop_diagnostic`: Shared diagnostic slot owned by the command
    ///   adapter.
    pub(crate) fn new(
        inner: OperationHandle<CommandOutput, CommandError>,
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

    /// Moves this handle into its asynchronous view.
    #[cfg(feature = "async")]
    #[must_use = "retain the asynchronous operation handle"]
    pub fn into_async(self) -> crate::AsyncCommandOperation {
        crate::AsyncCommandOperation::new(self.inner.into_async(), self.stop_diagnostic)
    }

    /// Returns the underlying operation handle, discarding access to the
    /// command-specific stop diagnostic slot.
    #[must_use = "retain the underlying operation handle"]
    pub fn into_inner(self) -> OperationHandle<CommandOutput, CommandError> {
        self.inner
    }
}

impl Deref for CommandOperation {
    /// The upstream operation handle type.
    type Target = OperationHandle<CommandOutput, CommandError>;

    /// Borrows the upstream operation handle.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for CommandOperation {
    /// Mutably borrows the upstream operation handle.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
