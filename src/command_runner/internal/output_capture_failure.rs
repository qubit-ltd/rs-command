// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Failure details retained after separating partial output from a failed
//! output reader.

use std::io;
use std::path::PathBuf;

/// Error details retained after separating partial output from a failed
/// output reader.
pub(in crate::command_runner) enum OutputCaptureFailure {
    /// The child pipe could not be read.
    Read {
        /// Operating-system read error.
        source: io::Error,
    },
    /// Writing retained output to a tee failed.
    Write {
        /// Configured tee path.
        path: PathBuf,
        /// Operating-system write error.
        source: io::Error,
    },
}
