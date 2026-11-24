// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "async")]
use std::sync::Arc;

#[cfg(feature = "async")]
use qubit_operation::OperationContext;

/// Carries optional session resources and a scope deadline during async
/// admission.
#[cfg(feature = "async")]
pub(in crate::execution) struct AsyncSubmissionContext {
    /// Parent-directory basis reused when a capacity waiter retries submission.
    pub(in crate::execution) io_path_base: Option<crate::execution::io_path_base::IoPathBase>,
    /// Session bridge connected to the operation, when this is a session.
    pub(in crate::execution) bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    /// Session channel owners retained for the operation lifetime.
    pub(in crate::execution) keepalive: Option<super::super::adapter::SessionOwners>,
    /// Optional deadline inherited from an operation scope.
    pub(in crate::execution) deadline: Option<OperationContext>,
}

#[cfg(feature = "async")]
impl AsyncSubmissionContext {
    /// Builds a non-session admission context with an optional scope deadline.
    ///
    /// # Parameters
    /// * `deadline`: Optional operation scope context applied during admission.
    pub(in crate::execution) fn plain(deadline: Option<OperationContext>) -> Self {
        Self {
            io_path_base: None,
            bridge: None,
            keepalive: None,
            deadline,
        }
    }

    /// Uses a path basis captured when a waiting submission first ran.
    ///
    /// # Parameters
    /// * `base`: Parent-directory result shared across retries.
    pub(in crate::execution) fn with_io_path_base(mut self, base: crate::execution::io_path_base::IoPathBase) -> Self {
        self.io_path_base = Some(base);
        self
    }
}
