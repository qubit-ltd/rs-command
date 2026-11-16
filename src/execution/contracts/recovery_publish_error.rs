// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// The adapter could not retain an unresolved owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryPublishError {
    /// Describes the store unavailable case handled by managed command
    /// execution.
    StoreUnavailable,
    /// Describes the capacity exceeded case handled by managed command
    /// execution.
    CapacityExceeded,
}
