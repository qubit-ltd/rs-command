// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Identifies why a cleanup owner could not enter the reaper queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HandoffFailureKind {
    /// The bounded queue has no available slot; retrying may succeed.
    Full,
    /// The queue's worker pool has stopped and cannot accept owners.
    Disconnected,
    /// A new worker pool could not be started.
    StartFailed,
}
