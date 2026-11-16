// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Mutex;

use crate::execution::contracts::FilePreparationFailure;
use crate::execution::contracts::RecoveryOwner;

/// Represents the preparation recovery owner state or contract used by managed
/// command execution.
pub(super) struct PreparationRecoveryOwner {
    /// Stores the retained value used by this execution component.
    retained: Mutex<Option<(FilePreparationFailure, qubit_operation::OperationScope)>>,
    /// Stores the  capacity lease value used by this execution component.
    _capacity_lease: Mutex<Option<crate::execution::contracts::ReservationLease>>,
}

impl RecoveryOwner for PreparationRecoveryOwner {
    /// Performs the summary step in the managed command execution lifecycle.
    fn summary(&self) -> String {
        "command file preparation retained partial resources".to_owned()
    }

    /// Stores the retry cleanup value used by this execution component.
    fn retry_cleanup(&self) -> Result<(), crate::execution::contracts::RecoveryRetryError> {
        let mut retained = self.retained.lock().unwrap_or_else(|error| error.into_inner());
        let Some((failure, scope)) = retained.take() else {
            return Err(crate::execution::contracts::RecoveryRetryError::OwnerAlreadyConsumed);
        };
        drop(failure);
        drop(scope);
        // Keep the capacity lease alive until T6 observes the upstream scope's final
        // settlement and removes this accepted recovery owner from its store.
        Ok(())
    }

    /// Performs the is complete step in the managed command execution
    /// lifecycle.
    fn is_complete(&self) -> bool {
        // This is only the owner handoff state. T6 must continue retaining this owner
        // (and its capacity lease) until the upstream scope report is observed;
        // only that report proves the partial resource cleanup outcome.
        self.retained
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_none()
    }
}
