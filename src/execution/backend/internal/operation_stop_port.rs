// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use qubit_operation::OperationId;
use qubit_operation::StopReason;

use crate::execution::backend::StartGate;

/// Represents the operation stop port state or contract used by managed command
/// execution.
pub(crate) struct OperationStopPort {
    /// Stores the operation id value used by this execution component.
    pub(in crate::execution::backend) operation_id: OperationId,
    /// Stores the gate value used by this execution component.
    pub(in crate::execution::backend) gate: Arc<StartGate>,
    /// Stores the gates value used by this execution component.
    pub(in crate::execution::backend) gates:
        std::sync::Weak<Mutex<HashMap<OperationId, Arc<StartGate>>>>,
}

impl crate::execution::contracts::StopPort for OperationStopPort {
    /// Stores the request stop value used by this execution component.
    ///
    /// # Parameters
    /// * `reason`: Supplies the reason value used by this operation.
    fn request_stop(&self, reason: StopReason) {
        self.gate.request_stop(reason);
    }
}

impl Drop for OperationStopPort {
    /// Performs the drop step in the managed command execution lifecycle.
    fn drop(&mut self) {
        if let Some(gates) = self.gates.upgrade() {
            gates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.operation_id);
        }
    }
}
