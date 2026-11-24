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

#[cfg(all(test, feature = "async"))]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::CommandOperation;
    use crate::CommandError;
    use crate::CommandErrorReason;
    use crate::CommandOutput;

    fn failed_handle(manager: &OperationManager) -> qubit_operation::OperationHandle<CommandOutput, CommandError> {
        manager
            .try_submit(OperationOptions::default(), |_| {
                Err::<CommandOutput, _>(CommandError::from_reason(
                    "operation command",
                    CommandErrorReason::Cancelled,
                    None,
                ))
            })
            .expect("operation should be accepted")
    }

    #[test]
    fn test_async_conversion_preserves_one_shot_stop_diagnostic_and_outcome() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let diagnostic = CommandError::from_reason("retained stop diagnostic", CommandErrorReason::Cancelled, None);
        let slot = Arc::new(Mutex::new(Some(diagnostic)));
        let operation = CommandOperation::new(failed_handle(&manager), Arc::clone(&slot));

        let asynchronous = operation.into_async();
        let mut synchronous = asynchronous.into_sync();
        assert_eq!(
            synchronous.take_stop_diagnostic().unwrap().command(),
            "retained stop diagnostic"
        );
        assert!(synchronous.take_stop_diagnostic().is_none());
        assert!(slot.lock().unwrap().is_none());

        let OperationOutcome::Failed(failure) = synchronous.wait_outcome().unwrap() else {
            panic!("operation should preserve its failed outcome");
        };
        assert_eq!(failure.command(), "operation command");
        synchronous.wait_settlement().unwrap();
    }

    #[test]
    fn test_sync_take_stop_diagnostic_recovers_poisoned_slot() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let diagnostic = CommandError::from_reason("poisoned retained diagnostic", CommandErrorReason::Cancelled, None);
        let slot = Arc::new(Mutex::new(Some(diagnostic)));
        let poison_slot = Arc::clone(&slot);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poison_slot.lock().unwrap();
            panic!("poison the diagnostic slot while it retains the diagnostic");
        }));
        assert!(slot.is_poisoned());

        let mut operation = CommandOperation::new(failed_handle(&manager), Arc::clone(&slot));
        assert_eq!(
            operation.take_stop_diagnostic().unwrap().command(),
            "poisoned retained diagnostic"
        );
        assert!(operation.take_stop_diagnostic().is_none());
        assert!(slot.lock().unwrap_err().into_inner().is_none());

        let OperationOutcome::Failed(failure) = operation.wait_outcome().unwrap() else {
            panic!("operation should preserve its failed outcome");
        };
        assert_eq!(failure.command(), "operation command");
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_into_inner_preserves_the_operation_result() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let slot = Arc::new(Mutex::new(Some(CommandError::from_reason(
            "unobserved diagnostic",
            CommandErrorReason::Cancelled,
            None,
        ))));
        let operation = CommandOperation::new(failed_handle(&manager), slot);

        let mut inner = operation.into_inner();
        let OperationOutcome::Failed(failure) = inner.wait_outcome().unwrap() else {
            panic!("inner handle should preserve its failed outcome");
        };
        assert_eq!(failure.command(), "operation command");
        inner.wait_settlement().unwrap();
    }
}
