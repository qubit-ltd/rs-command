// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Secondary cleanup failures retained alongside a command's primary error.

use std::io;
use std::path::PathBuf;

/// Failure observed while cleaning up after a command's primary result was
/// already determined.
#[derive(Debug)]
#[non_exhaustive]
pub enum CommandCleanupFailure {
    /// Waiting for the final child status failed.
    Wait {
        /// Operating-system wait error.
        source: io::Error,
    },
    /// Terminating the process tree failed.
    ProcessTreeTermination {
        /// Operating-system process-tree termination error.
        source: io::Error,
    },
    /// Terminating the direct child failed.
    ChildTermination {
        /// Operating-system direct-child termination error.
        source: io::Error,
    },
    /// The stdin helper failed during cleanup.
    Stdin {
        /// Stdin helper cleanup error.
        source: io::Error,
    },
    /// The stdout reader failed during cleanup.
    StdoutRead {
        /// Stdout reader cleanup error.
        source: io::Error,
    },
    /// The stdout tee failed during cleanup.
    StdoutWrite {
        /// Configured stdout tee path.
        path: PathBuf,
        /// Stdout tee cleanup error.
        source: io::Error,
    },
    /// The stderr reader failed during cleanup.
    StderrRead {
        /// Stderr reader cleanup error.
        source: io::Error,
    },
    /// The stderr tee failed during cleanup.
    StderrWrite {
        /// Configured stderr tee path.
        path: PathBuf,
        /// Stderr tee cleanup error.
        source: io::Error,
    },
}
