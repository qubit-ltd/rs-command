// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::RecoveryOwner;
use super::RecoveryPublishError;

/// Rejected transfer that returns ownership to the backend for retry or reaper
/// handoff.
pub struct RecoveryPublishFailure {
    /// Stores the reason value used by this execution component.
    pub reason: RecoveryPublishError,
    /// Stores the owner value used by this execution component.
    pub owner: Box<dyn RecoveryOwner>,
}

impl std::fmt::Debug for RecoveryPublishFailure {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `formatter`: Supplies the formatter value used by this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryPublishFailure")
            .field("reason", &self.reason)
            .field("owner_summary", &self.owner.summary())
            .finish()
    }
}
