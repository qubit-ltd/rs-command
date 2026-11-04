// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use super::output_tee::OutputTee;

/// An opened tee file paired with the path used in diagnostics.
pub(in crate::command_runner) struct FileTee {
    /// Opened file that receives the captured output stream.
    file: File,
    /// Original configured path used in write diagnostics.
    path: PathBuf,
}

impl FileTee {
    /// Creates a tee from its opened file and diagnostic path.
    ///
    /// # Parameters
    ///
    /// * `file` - Opened output destination validated for this tee.
    /// * `path` - Configured path used to identify the destination in
    ///   diagnostics.
    pub(in crate::command_runner) fn new(file: File, path: PathBuf) -> Self {
        Self { file, path }
    }

    /// Returns the opened tee file.
    pub(in crate::command_runner) fn file(&self) -> &File {
        &self.file
    }

    /// Returns the path used for tee diagnostics.
    pub(in crate::command_runner) fn path(&self) -> &Path {
        &self.path
    }

    /// Converts this file tee into an output reader tee.
    pub(in crate::command_runner) fn into_output_tee(self) -> OutputTee {
        OutputTee::new(Box::new(self.file), self.path)
    }
}
