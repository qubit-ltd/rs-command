// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Immutable parent-directory base captured at submission for relative path
/// I/O.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct ExecutionSnapshot {
    /// Parent process directory used to resolve managed stdin and tee paths.
    pub working_directory: PathBuf,
    /// Compatibility field retained for callers. Production requests leave it
    /// empty; child environment inheritance is resolved by the process at
    /// spawn time.
    pub environment: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
}

impl std::fmt::Debug for ExecutionSnapshot {
    /// Formats only whether a working directory is present and the environment
    /// entry count.
    ///
    /// # Parameters
    /// * `self`: Snapshot whose redacted metadata is formatted.
    /// * `formatter`: Destination for the debug representation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExecutionSnapshot")
            .field(
                "working_directory",
                &(!self.working_directory.as_os_str().is_empty()).then_some("<redacted path>"),
            )
            .field("environment_entries", &self.environment.len())
            .finish()
    }
}
