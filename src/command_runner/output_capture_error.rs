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
    io,
    path::PathBuf,
};

/// Error reported by an output reader thread.
#[derive(Debug)]
pub(crate) enum OutputCaptureError {
    /// Reading from the child pipe failed.
    Read(io::Error),
    /// Writing to a tee file failed.
    Write {
        /// Tee file path.
        path: PathBuf,
        /// I/O error reported by the writer.
        source: io::Error,
    },
}
