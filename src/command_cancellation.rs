// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// A one-shot cancellation handle for a running
/// [`CommandRunner`](crate::CommandRunner).
///
/// Clone this handle before passing it to
/// [`CommandRunOptions::cancellation`](crate::CommandRunOptions::cancellation).
/// Cancellation is a cooperative request, not a guarantee that the run returns
/// [`CommandErrorKind::Cancelled`](crate::CommandErrorKind::Cancelled). A
/// request observed before the final startup check makes the runner return a
/// [`CommandError`](crate::CommandError) with kind
/// [`CommandErrorKind::CancelledBeforeStart`](crate::CommandErrorKind::CancelledBeforeStart)
/// without creating or truncating tee files or spawning the command. After
/// startup, the runner checks for the request while monitoring the child and
/// I/O helpers. If they have already completed before the request is observed,
/// the run may return its normal result. The handle is intentionally one-shot
/// and cannot be reset.
///
/// # Examples
///
/// ```
/// use qubit_command::{Command, CommandCancellation, CommandRunOptions, CommandRunner};
///
/// let cancellation = CommandCancellation::new();
/// let output = CommandRunner::without_timeout()
///     .run_with(
///         Command::new("rustc").arg("--version"),
///         CommandRunOptions::new().cancellation(cancellation.clone()),
///     )
///     .expect("rustc should run");
/// assert!(!output.stdout().is_empty());
/// assert!(!cancellation.is_cancelled());
/// ```
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
    /// Cancellation is an idempotent cooperative request. A runner observes
    /// it between wait polls while monitoring the child and I/O helpers. If
    /// they have already completed before the request is observed, the run may
    /// return its normal result.
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
