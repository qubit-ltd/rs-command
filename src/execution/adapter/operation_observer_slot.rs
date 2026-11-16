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
    fn wait(&self) -> OperationObserver {
        let mut observer = self.observer.lock().unwrap_or_else(|error| error.into_inner());
        while observer.is_none() {
            observer = self.available.wait(observer).unwrap_or_else(|error| error.into_inner());
        }
        observer.as_ref().expect("observer is set").clone()
    }
}
