// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Concrete diagnostic retained when submission snapshots cannot be captured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotFailure {
    /// Stores the diagnostic value used by this execution component.
    pub diagnostic: String,
}
