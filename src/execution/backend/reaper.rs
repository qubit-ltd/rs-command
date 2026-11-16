//! Explicit owner handoff for process/helper cleanup.
//!
//! A reaper owner is deliberately returned on failed transfer so callers can
//! keep observing and retrying cleanup without dropping the child resources.
use std::sync::mpsc::{SyncSender, TrySendError};

#[derive(Debug)]
pub(crate) struct ReaperOwner<T>(T);

impl<T> ReaperOwner<T> {
    pub(crate) fn new(owner: T) -> Self {
        Self(owner)
    }
    pub(crate) fn into_inner(self) -> T {
        self.0
    }
}

#[derive(Debug)]
pub(crate) struct Reaper<T> {
    sender: SyncSender<ReaperOwner<T>>,
}

impl<T> Clone for Reaper<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<T> Reaper<T> {
    pub(crate) fn from_sender(sender: SyncSender<ReaperOwner<T>>) -> Self {
        Self { sender }
    }

    pub(crate) fn handoff(&self, owner: ReaperOwner<T>) -> Result<(), ReaperOwner<T>> {
        match self.sender.try_send(owner) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(owner) | TrySendError::Disconnected(owner)) => Err(owner),
        }
    }
}
