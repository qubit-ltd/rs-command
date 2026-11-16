// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsString;
use std::path::PathBuf;

/// Immutable working-directory and environment view captured at submission.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionSnapshot {
    /// Stores the working directory value used by this execution component.
    pub working_directory: PathBuf,
    /// Stores the environment value used by this execution component.
    pub environment: Vec<(OsString, Option<OsString>)>,
}
