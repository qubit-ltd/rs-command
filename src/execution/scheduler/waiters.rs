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
    pub fn update(&mut self, id: u64, waker: &Waker) {
        if let Some((_, slot)) = self.entries.iter_mut().find(|(key, _)| *key == id) {
            *slot = Some(waker.clone());
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
