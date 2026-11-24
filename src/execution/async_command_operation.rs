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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::AsyncCommandOperation;
    use crate::CommandError;
    use crate::CommandErrorReason;
    use crate::CommandOutput;

    fn failed_handle(manager: &OperationManager) -> qubit_operation::OperationHandle<CommandOutput, CommandError> {
        manager
            .try_submit(OperationOptions::default(), |_| {
                Err::<CommandOutput, _>(CommandError::from_reason(
                    "async operation command",
                    CommandErrorReason::Cancelled,
                    None,
                ))
            })
            .expect("operation should be accepted")
    }

    #[test]
    fn test_async_handle_takes_stop_diagnostic_once_before_sync_conversion() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let diagnostic = CommandError::from_reason("async retained diagnostic", CommandErrorReason::Cancelled, None);
        let slot = Arc::new(Mutex::new(Some(diagnostic)));
        let mut operation = AsyncCommandOperation::new(failed_handle(&manager).into_async(), Arc::clone(&slot));

        assert_eq!(
            operation.take_stop_diagnostic().unwrap().command(),
            "async retained diagnostic"
        );
        assert!(operation.take_stop_diagnostic().is_none());
        assert!(slot.lock().unwrap().is_none());

        let mut synchronous = operation.into_sync();
        let OperationOutcome::Failed(failure) = synchronous.wait_outcome().unwrap() else {
            panic!("operation should preserve its failed outcome");
        };
        assert_eq!(failure.command(), "async operation command");
        synchronous.wait_settlement().unwrap();
    }

    #[test]
    fn test_async_take_stop_diagnostic_recovers_poisoned_slot() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let diagnostic = CommandError::from_reason("poisoned async diagnostic", CommandErrorReason::Cancelled, None);
        let slot = Arc::new(Mutex::new(Some(diagnostic)));
        let poison_slot = Arc::clone(&slot);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poison_slot.lock().unwrap();
            panic!("poison the async diagnostic slot while it retains the diagnostic");
        }));
        assert!(slot.is_poisoned());

        let mut operation = AsyncCommandOperation::new(failed_handle(&manager).into_async(), Arc::clone(&slot));
        assert_eq!(
            operation.take_stop_diagnostic().unwrap().command(),
            "poisoned async diagnostic"
        );
        assert!(operation.take_stop_diagnostic().is_none());
        assert!(slot.lock().unwrap_err().into_inner().is_none());

        let mut synchronous = operation.into_sync();
        let OperationOutcome::Failed(failure) = synchronous.wait_outcome().unwrap() else {
            panic!("operation should preserve its failed outcome");
        };
        assert_eq!(failure.command(), "async operation command");
        synchronous.wait_settlement().unwrap();
    }

    #[test]
    fn test_async_into_inner_preserves_operation_result_and_settlement() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let operation = AsyncCommandOperation::new(failed_handle(&manager).into_async(), Arc::new(Mutex::new(None)));

        let mut inner = operation.into_inner().into_sync();
        let OperationOutcome::Failed(failure) = inner.wait_outcome().unwrap() else {
            panic!("inner handle should preserve its failed outcome");
        };
        assert_eq!(failure.command(), "async operation command");
        inner.wait_settlement().unwrap();
    }
}
