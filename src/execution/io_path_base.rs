// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Parent-directory result captured once for a managed submission that may
/// retry.
#[derive(Clone)]
pub(in crate::execution) struct IoPathBase {
    /// Captured current-directory value or its original lookup error.
    result: Result<std::path::PathBuf, String>,
}

/// Creates and reads a reusable parent-directory basis.
impl IoPathBase {
    /// Captures the current directory to anchor relative I/O paths across
    /// retries.
    pub(in crate::execution) fn capture() -> Self {
        Self {
            result: std::env::current_dir().map_err(|error| error.to_string()),
        }
    }

    /// Creates a basis from a previously captured current-directory result.
    ///
    /// # Parameters
    /// * `result`: Captured directory or the error produced while reading it.
    #[cfg(test)]
    pub(in crate::execution) fn from_result(result: Result<std::path::PathBuf, String>) -> Self {
        Self { result }
    }

    /// Returns the captured current-directory result.
    pub(in crate::execution) fn result(&self) -> Result<std::path::PathBuf, String> {
        self.result.clone()
    }
}
