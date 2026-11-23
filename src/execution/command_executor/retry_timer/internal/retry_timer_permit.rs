// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! RAII permit for a queued or pending retry timer.
/// Holds one shared in-flight retry timer slot until the message is completed
/// or discarded.
pub(super) struct RetryTimerPermit(
    /// Shared count decremented when this message leaves the queue or pending
    /// set.
    pub(super) std::sync::Arc<std::sync::atomic::AtomicUsize>,
);

impl Drop for RetryTimerPermit {
    /// Releases the shared in-flight slot.
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}
