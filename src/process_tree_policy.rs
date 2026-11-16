// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Process lifetime policy for managed command execution.

/// Controls whether managed execution owns the process tree or only its direct
/// child.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProcessTreePolicy {
    /// Manage descendants as part of the command lifetime.
    #[default]
    Managed,
    /// Stop and wait for only the direct child.
    DirectChild,
}
