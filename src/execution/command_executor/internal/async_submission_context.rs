// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

#[cfg(feature = "async")]
use std::sync::Arc;
#[cfg(feature = "async")]
use qubit_operation::OperationContext;

/// Carries optional session resources and a scope deadline during async admission.
#[cfg(feature = "async")]
pub(in crate::execution) struct AsyncSubmissionContext {
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
    pub(in crate::execution) fn plain(deadline: Option<OperationContext>) -> Self {
        Self {
            bridge: None,
            keepalive: None,
            deadline,
        }
    }
}
