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
