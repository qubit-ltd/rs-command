// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Publication rejected because an invariant would be violated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactPublishError {
    /// Describes the duplicate outcome case handled by managed command
    /// execution.
    DuplicateOutcome,
    /// Describes the duplicate release case handled by managed command
    /// execution.
    DuplicateRelease,
    /// Describes the duplicate settlement case handled by managed command
    /// execution.
    DuplicateSettlement,
    /// Describes the identity mismatch case handled by managed command
    /// execution.
    IdentityMismatch,
}
