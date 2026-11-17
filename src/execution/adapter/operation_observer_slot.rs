// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Condvar;
use std::sync::Mutex;

use qubit_operation::OperationObserver;

/// Represents the operation observer slot state or contract used by managed
/// command execution.
#[derive(Default)]
pub(crate) struct OperationObserverSlot {
    /// Stores the observer value used by this execution component.
    observer: Mutex<Option<OperationObserver>>,
    /// Stores the available value used by this execution component.
    available: Condvar,
}
impl OperationObserverSlot {
    /// Stores the set value used by this execution component.
    ///
    /// # Parameters
    /// * `observer`: Supplies the observer value used by this operation.
    pub(crate) fn set(&self, observer: OperationObserver) {
        *self.observer.lock().unwrap_or_else(|error| error.into_inner()) = Some(observer);
        self.available.notify_all();
    }

    /// Stores the wait value used by this execution component.
    pub(crate) fn wait(&self) -> OperationObserver {
        let mut observer = self.observer.lock().unwrap_or_else(|error| error.into_inner());
        while observer.is_none() {
            observer = self.available.wait(observer).unwrap_or_else(|error| error.into_inner());
        }
        observer.as_ref().expect("observer is set").clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;

    use super::OperationObserverSlot;

    #[test]
    fn waiting_observer_receives_published_operation_identity() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let expected_id = operation.id();
        let slot = Arc::new(OperationObserverSlot::default());
        let waiting_slot = Arc::clone(&slot);
        let (started_tx, started_rx) = mpsc::channel();
        let (observer_tx, observer_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            observer_tx.send(waiting_slot.wait()).unwrap();
        });

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        std::thread::sleep(Duration::from_millis(10));
        slot.set(operation.observer());
        let observed = observer_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        waiter.join().unwrap();
        assert_eq!(observed.id(), expected_id);
        let _outcome = operation.wait_outcome().unwrap();
        operation.wait_settlement().unwrap();
    }
}
