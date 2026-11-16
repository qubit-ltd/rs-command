// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Bounded, redacted reason that a recovery cleanup attempt could not be handed
/// off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryRetryError {
    /// Describes the owner already consumed case handled by managed command
    /// execution.
    OwnerAlreadyConsumed,
    /// Describes the cleanup handoff unavailable case handled by managed
    /// command execution.
    CleanupHandoffUnavailable,
}
