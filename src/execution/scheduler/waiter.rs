use std::task::Waker;

#[derive(Default)]
pub(super) struct Waiters {
    next: u64,
    entries: Vec<(u64, Option<Waker>)>,
}
impl Waiters {
    pub fn insert(&mut self, waker: Option<&Waker>, limit: usize) -> Option<u64> {
        if self.entries.len() >= limit {
            return None;
        }
        self.next = self.next.wrapping_add(1).max(1);
        self.entries.push((self.next, waker.cloned()));
        Some(self.next)
    }
    pub fn update(&mut self, id: u64, waker: &Waker) {
        if let Some((_, slot)) = self.entries.iter_mut().find(|(key, _)| *key == id) {
            *slot = Some(waker.clone());
        }
    }
    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|(key, _)| *key != id);
    }
    pub fn wake_all(&mut self) -> Vec<Waker> {
        self.entries.iter_mut().filter_map(|(_, w)| w.take()).collect()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
