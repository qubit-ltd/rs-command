/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::{
    fs::File,
    path::PathBuf,
};

use super::output_tee::OutputTee;

/// Output capture options moved into a reader thread.
pub(crate) struct OutputCaptureOptions {
    /// Maximum bytes retained in memory.
    pub(crate) max_bytes: Option<usize>,
    /// Optional writer receiving a streaming copy.
    pub(crate) tee: Option<OutputTee>,
}

impl OutputCaptureOptions {
    /// Creates output capture options.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Optional in-memory retention limit.
    /// * `file` - Optional file receiving all emitted bytes.
    /// * `file_path` - File path used in write-failure diagnostics.
    ///
    /// # Returns
    ///
    /// Capture options moved into the output reader thread.
    pub(crate) fn new(
        max_bytes: Option<usize>,
        file: Option<File>,
        file_path: Option<PathBuf>,
    ) -> Self {
        let tee = file.map(|file| OutputTee {
            writer: Box::new(file),
            path: file_path.unwrap_or_default(),
        });
        Self { max_bytes, tee }
    }
}
