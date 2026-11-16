// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Lexical I/O paths captured with the request; file identity is checked later
/// by preparation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IoPathSnapshot {
    /// Stores the stdin value used by this execution component.
    pub stdin: Option<PathBuf>,
    /// Stores the stdout value used by this execution component.
    pub stdout: Option<PathBuf>,
    /// Stores the stderr value used by this execution component.
    pub stderr: Option<PathBuf>,
}
