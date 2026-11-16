// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::OperationContext;

use crate::execution::scheduler::kind::Kind;

/// Represents the record state or contract used by managed command execution.
#[derive(Clone)]
pub(in crate::execution::scheduler) struct Record {
    /// Stores the kind value used by this execution component.
    pub(in crate::execution::scheduler) kind: Kind,
    /// Stores the bytes value used by this execution component.
    pub(in crate::execution::scheduler) bytes: usize,
    /// Stores the deadline value used by this execution component.
    pub(in crate::execution::scheduler) deadline: Option<OperationContext>,
    /// Stores the owner live value used by this execution component.
    pub(in crate::execution::scheduler) owner_live: bool,
    /// Stores the leases value used by this execution component.
    pub(in crate::execution::scheduler) leases: usize,
}
