//! Shared ownership for resources referenced by process helpers.
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct ResourceOwner<T>(Arc<T>);

impl<T> Clone for ResourceOwner<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> ResourceOwner<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Arc::new(value))
    }
    pub(crate) fn get(&self) -> &T {
        &self.0
    }
    pub(crate) fn strong_count(&self) -> usize {
        Arc::strong_count(&self.0)
    }
}
