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

    fn completed_observer() -> (qubit_operation::OperationObserver, qubit_operation::OperationId) {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let observer = handle.observer();
        let id = handle.id();
        let _ = handle.wait_outcome().unwrap();
        handle.wait_settlement().unwrap();
        (observer, id)
    }

    #[test]
    fn test_wait_blocks_until_observer_is_published_and_returns_its_identity() {
        let (observer, expected_id) = completed_observer();
        let slot = Arc::new(OperationObserverSlot::default());
        let waiter_slot = Arc::clone(&slot);
        let (started_tx, started_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            result_tx.send(waiter_slot.wait().id()).unwrap();
        });

        started_rx.recv().unwrap();
        assert!(result_rx.recv_timeout(Duration::from_millis(20)).is_err());
        slot.set(observer);

        assert_eq!(result_rx.recv_timeout(Duration::from_secs(1)).unwrap(), expected_id);
        assert_eq!(waiter.join().unwrap(), ());
        assert_eq!(slot.wait().id(), expected_id);
    }

    #[test]
    fn test_set_notifies_all_waiters_with_the_published_observer() {
        let (observer, expected_id) = completed_observer();
        let slot = Arc::new(OperationObserverSlot::default());
        let waiter_count = 3;
        let (started_tx, started_rx) = mpsc::channel();
        let mut waiters = Vec::new();

        for _ in 0..waiter_count {
            let waiter_slot = Arc::clone(&slot);
            let started_tx = started_tx.clone();
            waiters.push(std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                waiter_slot.wait().id()
            }));
        }
        drop(started_tx);
        for _ in 0..waiter_count {
            started_rx.recv().unwrap();
        }

        slot.set(observer);

        for waiter in waiters {
            assert_eq!(waiter.join().unwrap(), expected_id);
        }
    }

    #[test]
    fn test_observer_slot_recovers_a_poisoned_mutex() {
        let slot = OperationObserverSlot::default();
        let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _observer = slot.observer.lock().unwrap();
            panic!("inject poisoned observer slot");
        }));
        assert!(poison.is_err());

        let (observer, expected_id) = completed_observer();
        slot.set(observer);

        assert_eq!(slot.wait().id(), expected_id);
    }

    #[test]
    fn test_wait_recovers_poisoned_mutex_after_condvar_wait() {
        let slot = Arc::new(OperationObserverSlot::default());
        let waiter_slot = Arc::clone(&slot);
        let (started_tx, started_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            waiter_slot.wait().id()
        });
        started_rx.recv().unwrap();

        // Observe the waiter holding the mutex, then acquire it only after the
        // Condvar wait releases it. This makes poisoning happen while the
        // waiter is parked in `Condvar::wait`, without timing sleeps.
        let mut saw_waiter_lock = false;
        loop {
            match slot.observer.try_lock() {
                Ok(observer) if saw_waiter_lock => {
                    drop(observer);
                    break;
                }
                Ok(observer) => drop(observer),
                Err(std::sync::TryLockError::WouldBlock) => saw_waiter_lock = true,
                Err(std::sync::TryLockError::Poisoned(_)) => panic!("observer lock poisoned too early"),
            }
            std::thread::yield_now();
        }

        let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _observer = slot.observer.try_lock().expect("waiter released observer mutex");
            panic!("poison observer mutex while waiter is on its condition variable");
        }));
        assert!(poison.is_err());

        let (observer, expected_id) = completed_observer();
        slot.set(observer);
        assert_eq!(waiter.join().unwrap(), expected_id);
    }
}
