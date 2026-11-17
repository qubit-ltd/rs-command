// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::OperationScope;

use super::ExecutionSnapshot;
use super::FilePreparationFailure;
use super::PreparedFiles;

/// Resource inputs consumed by preparation.
pub trait ResourceInput: Send {
    /// Performs the prepare files step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    /// * `scope`: Supplies the scope value used by this operation.
    fn prepare_files(self: Box<Self>, scope: &OperationScope) -> Result<PreparedFiles, FilePreparationFailure>;

    /// Prepares files using the immutable submission-time path/environment
    /// snapshot.
    ///
    /// Implementations with path-based identity checks should override this
    /// method. The default preserves compatibility for resource inputs that
    /// only contain owned handles.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `_snapshot`: Supplies the  snapshot value used by this operation.
    fn prepare_files_with_snapshot(
        self: Box<Self>,
        scope: &OperationScope,
        _snapshot: &ExecutionSnapshot,
    ) -> Result<PreparedFiles, FilePreparationFailure> {
        self.prepare_files(scope)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    use qubit_operation::GroupOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::OperationScope;

    use super::ResourceInput;
    use crate::execution::contracts::ExecutionSnapshot;
    use crate::execution::contracts::FilePreparationFailure;
    use crate::execution::contracts::PreparedFiles;

    struct LegacyInput(Arc<AtomicBool>);

    impl ResourceInput for LegacyInput {
        fn prepare_files(
            self: Box<Self>,
            _scope: &OperationScope,
        ) -> Result<PreparedFiles, FilePreparationFailure> {
            self.0.store(true, Ordering::SeqCst);
            Ok(PreparedFiles::new(None, None, None))
        }
    }

    #[test]
    fn test_snapshot_preparation_default_forwards_to_legacy_file_preparation() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let prepared_legacy = Arc::new(AtomicBool::new(false));
        let observed = prepared_legacy.clone();

        let completion = group
            .run_and_settle(OperationOptions::default(), move |scope| {
                let prepared = Box::new(LegacyInput(observed))
                    .prepare_files_with_snapshot(&scope, &ExecutionSnapshot::default())
                    .expect("default adapter should delegate legacy preparation");
                assert_eq!(
                    format!("{prepared:?}"),
                    "PreparedFiles { stdin: false, stdout: false, stderr: false }"
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            completion.outcome,
            OperationOutcome::Succeeded(())
        ));
        assert!(prepared_legacy.load(Ordering::SeqCst));
    }
}
