// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Test-only temporary directory compatibility helper.

use std::io::Result;
use std::path::Path;

/// Automatically removed temporary directory used by integration tests.
pub(crate) struct LocalTempDir(tempfile::TempDir);

impl LocalTempDir {
    /// Creates a temporary directory with a name prefix.
    pub(crate) fn with_prefix(prefix: &str) -> Result<Self> {
        tempfile::Builder::new().prefix(prefix).tempdir().map(Self)
    }

    /// Creates a temporary directory inside a selected parent.
    pub(crate) fn in_dir(directory: impl AsRef<Path>, prefix: Option<&str>, _max_tries: usize) -> Result<Self> {
        let mut builder = tempfile::Builder::new();
        if let Some(prefix) = prefix {
            builder.prefix(prefix);
        }
        builder.tempdir_in(directory).map(Self)
    }

    /// Returns the temporary directory path.
    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }
}
