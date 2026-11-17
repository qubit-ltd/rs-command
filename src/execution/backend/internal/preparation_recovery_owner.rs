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
pub(crate) struct PreparationRecoveryOwner {
    /// Stores the retained value used by this execution component.
    pub(in crate::execution::backend) retained:
        Mutex<Option<(FilePreparationFailure, qubit_operation::OperationScope)>>,
    /// Stores the  capacity lease value used by this execution component.
    pub(in crate::execution::backend) _capacity_lease: Mutex<Option<crate::execution::contracts::ReservationLease>>,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::PreparationRecoveryOwner;
    use crate::execution::contracts::FilePreparationFailure;
    use crate::execution::contracts::RecoveryOwner;
    use crate::execution::contracts::RecoveryRetryError;
    use crate::execution::contracts::ReservationLease;

    #[test]
    fn test_retry_releases_scope_resources_but_retains_capacity_until_owner_drop() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let retained = Arc::new(Mutex::new(None));
        let retained_for_operation = Arc::clone(&retained);
        let (lease_released_tx, lease_released_rx) = std::sync::mpsc::channel();
        let mut operation = manager
            .submit(OperationOptions::default(), move |scope| {
                let owner = PreparationRecoveryOwner {
                    retained: Mutex::new(Some((
                        FilePreparationFailure {
                            message: "sink truncation failed after opening stdin".into(),
                            input: None,
                            prepared: None,
                        },
                        scope,
                    ))),
                    _capacity_lease: Mutex::new(Some(ReservationLease::new(move || {
                        lease_released_tx.send(()).unwrap();
                    }))),
                };
                *retained_for_operation.lock().unwrap() = Some(owner);
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let owner = retained
            .lock()
            .unwrap()
            .take()
            .expect("recovery owner should be retained");
        assert_eq!(owner.summary(), "command file preparation retained partial resources");
        assert!(!owner.is_complete());
        owner
            .retry_cleanup()
            .expect("first retry should release retained scope responsibility");
        assert!(owner.is_complete());
        assert_eq!(
            owner.retry_cleanup().unwrap_err(),
            RecoveryRetryError::OwnerAlreadyConsumed
        );
        assert!(
            lease_released_rx.try_recv().is_err(),
            "capacity stays leased until owner release"
        );

        drop(owner);
        lease_released_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("dropping recovery owner should release capacity");
        operation.wait_settlement().unwrap();
    }
}
