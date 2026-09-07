// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io::Write;
use std::path::PathBuf;

/// Streaming destination for captured output.
pub(in crate::command_runner) struct OutputTee {
    /// Writer receiving all emitted bytes.
    pub(in crate::command_runner) writer: Box<dyn Write + Send>,
    /// Path used for diagnostics if writes fail.
    pub(in crate::command_runner) path: PathBuf,
}

impl OutputTee {
    /// Creates a streaming destination with its diagnostic path.
    #[inline]
    pub(in crate::command_runner) fn new(writer: Box<dyn Write + Send>, path: PathBuf) -> Self {
        Self { writer, path }
    }
}
