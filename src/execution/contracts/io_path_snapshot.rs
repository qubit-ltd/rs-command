// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Resolved I/O targets captured with the request; file identity is checked
/// later by preparation. Relative paths use the parent directory captured in
/// [`super::ExecutionSnapshot`], independently of the child working directory.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct IoPathSnapshot {
    /// Stores the stdin value used by this execution component.
    pub stdin: Option<PathBuf>,
    /// Stores the stdout value used by this execution component.
    pub stdout: Option<PathBuf>,
    /// Stores the stderr value used by this execution component.
    pub stderr: Option<PathBuf>,
}

impl std::fmt::Debug for IoPathSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IoPathSnapshot")
            .field("stdin", &self.stdin.is_some())
            .field("stdout", &self.stdout.is_some())
            .field("stderr", &self.stderr.is_some())
            .finish()
    }
}
