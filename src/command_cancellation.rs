// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::{
    Arc,
    atomic::{
        AtomicBool,
        Ordering,
    },
};

/// A one-shot cancellation handle for a running
/// [`CommandRunner`](crate::CommandRunner).
///
/// Clone this handle before passing it to
/// [`CommandRunner::cancellation_token`](crate::CommandRunner::cancellation_token).
/// Calling [`Self::cancel`] makes the runner terminate its managed process tree
/// and return [`CommandError::Cancelled`](crate::CommandError::Cancelled). The
/// handle is intentionally one-shot and cannot be reset.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct CommandCancellation {
    /// Shared cancellation state observed by configured command runners.
    cancelled: Arc<AtomicBool>,
}

impl CommandCancellation {
    /// Creates a cancellation handle in the active state.
    ///
    /// # Returns
    ///
    /// A handle that has not been cancelled.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels all command runs configured with a clone of this handle.
    ///
    /// Cancellation is idempotent. A runner observes the request between wait
    /// polls, terminates its managed process tree, and returns a typed error.
    #[inline]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Reports whether cancellation has been requested.
    ///
    /// # Returns
    ///
    /// `true` after [`Self::cancel`] has been called on any clone of this
    /// handle, otherwise `false`.
    #[must_use]
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
