// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::sync::Mutex;

use qubit_operation::ResourceRef;

use super::PreparedSink;

/// Prepared files retain registered owners; helpers share files through
/// `Mutex<File>`.
pub struct PreparedFiles {
    /// Stores the stdin value used by this execution component.
    pub(crate) stdin: Option<ResourceRef<Mutex<File>>>,
    /// Stores the stdout value used by this execution component.
    pub(crate) stdout: Option<PreparedSink>,
    /// Stores the stderr value used by this execution component.
    pub(crate) stderr: Option<PreparedSink>,
}
impl PreparedFiles {
    /// Constructs a prepared owner set from resources registered in the
    /// operation scope.
    ///
    /// # Parameters
    /// * `stdin`: Supplies the stdin value used by this operation.
    /// * `stdout`: Supplies the stdout value used by this operation.
    /// * `stderr`: Supplies the stderr value used by this operation.
    pub(crate) fn new(
        stdin: Option<ResourceRef<Mutex<File>>>,
        stdout: Option<PreparedSink>,
        stderr: Option<PreparedSink>,
    ) -> Self {
        Self {
            stdin,
            stdout,
            stderr,
        }
    }
}
impl std::fmt::Debug for PreparedFiles {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `formatter`: Supplies the formatter value used by this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedFiles")
            .field("stdin", &self.stdin.is_some())
            .field("stdout", &self.stdout.is_some())
            .field("stderr", &self.stderr.is_some())
            .finish()
    }
}
