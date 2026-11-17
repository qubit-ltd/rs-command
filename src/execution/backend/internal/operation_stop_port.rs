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
    pub(in crate::execution::backend) gates: std::sync::Weak<Mutex<HashMap<OperationId, Arc<StartGate>>>>,
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::StopReason;

    use super::OperationStopPort;
    use crate::execution::backend::StartGate;
    use crate::execution::contracts::StopPort;

    #[test]
    fn test_stop_port_propagates_reason_and_removes_its_gate_when_dropped() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let operation_id = operation.id();
        let gate = std::sync::Arc::new(StartGate::default());
        let gates = std::sync::Arc::new(std::sync::Mutex::new(HashMap::from([(
            operation_id,
            std::sync::Arc::clone(&gate),
        )])));
        let port = OperationStopPort {
            operation_id,
            gate: std::sync::Arc::clone(&gate),
            gates: std::sync::Arc::downgrade(&gates),
        };

        StopPort::request_stop(&port, StopReason::Timeout);
        assert_eq!(gate.stop_reason(), Some(StopReason::Timeout));
        assert!(gate.cancellation().is_cancelled());

        drop(port);
        assert!(!gates.lock().unwrap().contains_key(&operation_id));
        assert!(operation.wait_outcome().is_ok());
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_drop_after_gate_registry_is_gone_is_harmless() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let gate = std::sync::Arc::new(StartGate::default());
        let gates = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let port = OperationStopPort {
            operation_id: operation.id(),
            gate,
            gates: std::sync::Arc::downgrade(&gates),
        };

        drop(gates);
        drop(port);

        assert!(operation.wait_outcome().is_ok());
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_drop_removes_gate_from_poisoned_registry() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let operation_id = operation.id();
        let gate = std::sync::Arc::new(StartGate::default());
        let gates = std::sync::Arc::new(std::sync::Mutex::new(HashMap::from([(
            operation_id,
            std::sync::Arc::clone(&gate),
        )])));
        let port = OperationStopPort {
            operation_id,
            gate,
            gates: std::sync::Arc::downgrade(&gates),
        };

        let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _registry = gates.lock().unwrap();
            panic!("simulate a panic while updating the operation gate registry");
        }));
        assert!(poison.is_err());
        assert!(gates.is_poisoned());

        drop(port);

        let registry = gates.lock().unwrap_or_else(|error| error.into_inner());
        assert!(!registry.contains_key(&operation_id));
        drop(registry);
        assert!(operation.wait_outcome().is_ok());
        operation.wait_settlement().unwrap();
    }
}
