// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Identifies why a cleanup owner was rejected by the reaper queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HandoffFailureKind {
    /// The bounded queue has no available slot. A later submission may succeed.
    Full,
    /// The worker pool has stopped and the queue has no receiver.
    Disconnected,
    /// Startup failed before a replacement worker pool became available.
    StartFailed,
}
