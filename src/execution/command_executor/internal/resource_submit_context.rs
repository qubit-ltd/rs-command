// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::OperationContext;

use crate::execution::io_path_base::IoPathBase;

/// Carries the deadline and path snapshot used by a resource submission.
pub(in crate::execution) struct ResourceSubmitContext<'a> {
    /// Optional operation deadline inherited by this submission.
    pub(in crate::execution) deadline: Option<OperationContext>,
    /// Parent-directory snapshot shared by retries, when present.
    pub(in crate::execution) base: Option<&'a IoPathBase>,
}
