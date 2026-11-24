// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::ReaperOwner;
use crate::execution::contracts::RecoveryOwner;

/// Holds a cleanup owner until the backend selects its handoff path.
pub(crate) enum RetainedOwner {
    /// Keeps an owner for later retry by the reaper recovery pool.
    Deferred {
        /// Holds the recovery owner until the pool accepts responsibility.
        owner: Box<dyn RecoveryOwner>,
    },
    /// Keeps command resources for cleanup after the outcome is published.
    Local {
        /// Holds resources that the backend must clean up locally.
        owner: ReaperOwner,
    },
}
