// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Message state transferred from retry tasks to the timer worker.
use super::retry_timer_permit::RetryTimerPermit;

/// Carries one retry deadline and its task notification state.
pub(super) struct RetryTimerMessage {
    /// The instant when the retry task becomes ready.
    pub(super) due: std::time::Instant,
    /// The most recently registered task waker.
    pub(super) waker: std::sync::Arc<std::sync::Mutex<Option<std::task::Waker>>>,
    /// Whether the deadline has elapsed.
    pub(super) ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Whether the retry task has been dropped.
    pub(super) cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The shared quota slot held by this queued or pending message.
    pub(super) permit: Option<RetryTimerPermit>,
}
