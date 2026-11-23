// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::task::Waker;

/// Represents the waiters state or contract used by managed command execution.
#[derive(Default)]
pub(super) struct Waiters {
    /// Stores the next value used by this execution component.
    next: u64,
    /// Stores the entries value used by this execution component.
    entries: Vec<(u64, Option<Waker>)>,
}
impl Waiters {
    /// Stores the insert value used by this execution component.
    ///
    /// # Parameters
    /// * `waker`: Supplies the waker value used by this operation.
    /// * `limit`: Supplies the limit value used by this operation.
    pub fn insert(&mut self, waker: Option<&Waker>, limit: usize) -> Option<u64> {
        if self.entries.len() >= limit {
            return None;
        }
        self.next = self.next.wrapping_add(1).max(1);
        self.entries.push((self.next, waker.cloned()));
        Some(self.next)
    }
    /// Stores the update value used by this execution component.
    ///
    /// # Parameters
    /// * `id`: Supplies the id value used by this operation.
    /// * `waker`: Supplies the waker value used by this operation.
    ///
    /// # Returns
    /// `true` when the waiter ID exists and was updated.
    pub fn update(&mut self, id: u64, waker: &Waker) -> bool {
        if let Some((_, slot)) = self.entries.iter_mut().find(|(key, _)| *key == id) {
            *slot = Some(waker.clone());
            true
        } else {
            false
        }
    }
    /// Stores the remove value used by this execution component.
    ///
    /// # Parameters
    /// * `id`: Supplies the id value used by this operation.
    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|(key, _)| *key != id);
    }
    /// Performs the wake all step in the managed command execution lifecycle.
    pub fn wake_all(&mut self) -> Vec<Waker> {
        self.entries.iter_mut().filter_map(|(_, w)| w.take()).collect()
    }
    /// Performs the len step in the managed command execution lifecycle.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use super::Waiters;

    struct CountWake(AtomicUsize);

    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_waiter_insert_enforces_limit_and_returns_unique_nonzero_ids() {
        let mut waiters = Waiters::default();
        let wake = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(wake);

        assert_eq!(waiters.insert(None, 0), None);
        let first = waiters.insert(Some(&waker), 2).unwrap();
        let second = waiters.insert(None, 2).unwrap();
        assert_ne!(first, 0);
        assert_ne!(second, 0);
        assert_ne!(first, second);
        assert_eq!(waiters.len(), 2);
        assert_eq!(waiters.insert(Some(&waker), 2), None);
    }

    #[test]
    fn test_waiter_update_replaces_waker_and_ignores_unknown_ids() {
        let mut waiters = Waiters::default();
        let old = Arc::new(CountWake(AtomicUsize::new(0)));
        let replacement = Arc::new(CountWake(AtomicUsize::new(0)));
        let old_waker = Waker::from(old.clone());
        let replacement_waker = Waker::from(replacement.clone());
        let id = waiters.insert(Some(&old_waker), 1).unwrap();

        waiters.update(id, &replacement_waker);
        waiters.update(id + 1, &old_waker);
        for waker in waiters.wake_all() {
            waker.wake();
        }

        assert_eq!(old.0.load(Ordering::SeqCst), 0);
        assert_eq!(replacement.0.load(Ordering::SeqCst), 1);
        assert!(waiters.wake_all().is_empty());
    }

    #[test]
    fn test_removing_waiter_prevents_wake_but_preserves_other_waiters() {
        let mut waiters = Waiters::default();
        let removed = Arc::new(CountWake(AtomicUsize::new(0)));
        let retained = Arc::new(CountWake(AtomicUsize::new(0)));
        let removed_waker = Waker::from(removed.clone());
        let retained_waker = Waker::from(retained.clone());
        let removed_id = waiters.insert(Some(&removed_waker), 2).unwrap();
        waiters.insert(Some(&retained_waker), 2).unwrap();

        waiters.remove(removed_id);
        waiters.remove(removed_id);
        assert_eq!(waiters.len(), 1);
        for waker in waiters.wake_all() {
            waker.wake();
        }

        assert_eq!(removed.0.load(Ordering::SeqCst), 0);
        assert_eq!(retained.0.load(Ordering::SeqCst), 1);
        assert_eq!(waiters.len(), 1);
    }

    #[test]
    fn test_waiter_waker_can_be_notified_by_reference() {
        let wake = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(wake.clone());

        waker.wake_by_ref();

        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        assert_eq!(Arc::strong_count(&wake), 2);
    }
}
