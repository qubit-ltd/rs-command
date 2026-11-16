// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;

use qubit_operation::OperationContext;

use super::Record;
use crate::execution::scheduler::waiters::Waiters;
use crate::execution::scheduler::worker::WorkerQueue;

/// Represents the state state or contract used by managed command execution.
pub(in crate::execution::scheduler) struct State {
    /// Stores the closed value used by this execution component.
    pub(in crate::execution::scheduler) closed: bool,
    /// Stores the next value used by this execution component.
    pub(in crate::execution::scheduler) next: u64,
    /// Stores the active value used by this execution component.
    pub(in crate::execution::scheduler) active: usize,
    /// Stores the queued value used by this execution component.
    pub(in crate::execution::scheduler) queued: usize,
    /// Stores the input bytes value used by this execution component.
    pub(in crate::execution::scheduler) input_bytes: usize,
    /// Stores the residual helpers value used by this execution component.
    pub(in crate::execution::scheduler) residual_helpers: usize,
    /// Stores the records value used by this execution component.
    pub(in crate::execution::scheduler) records: HashMap<u64, Record>,
    /// Stores the waiters value used by this execution component.
    pub(in crate::execution::scheduler) waiters: Waiters,
    /// Stores the workers value used by this execution component.
    pub(in crate::execution::scheduler) workers: WorkerQueue,
}
