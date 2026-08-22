// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Secondary cleanup failures retained alongside a command's primary error.

use std::fmt;
use std::io;
use std::path::PathBuf;

use qubit_redact::Redactor;

/// Redacts one debug-only value before it crosses the diagnostic boundary.
fn redacted_debug_text(value: &impl fmt::Debug) -> String {
    Redactor::strict()
        .redact_field("command_path", &format!("{value:?}"))
        .into_text()
        .into_string()
}

/// Failure observed while cleaning up after a command's primary result was
/// already determined.
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

impl fmt::Debug for CommandCleanupFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wait { source } => formatter
                .debug_struct("Wait")
                .field("source", source)
                .finish(),
            Self::ProcessTreeTermination { source } => formatter
                .debug_struct("ProcessTreeTermination")
                .field("source", source)
                .finish(),
            Self::ChildTermination { source } => formatter
                .debug_struct("ChildTermination")
                .field("source", source)
                .finish(),
            Self::Stdin { source } => formatter
                .debug_struct("Stdin")
                .field("source", source)
                .finish(),
            Self::StdoutRead { source } => formatter
                .debug_struct("StdoutRead")
                .field("source", source)
                .finish(),
            Self::StdoutWrite { path, source } => formatter
                .debug_struct("StdoutWrite")
                .field("path", &redacted_debug_text(path))
                .field("source", source)
                .finish(),
            Self::StderrRead { source } => formatter
                .debug_struct("StderrRead")
                .field("source", source)
                .finish(),
            Self::StderrWrite { path, source } => formatter
                .debug_struct("StderrWrite")
                .field("path", &redacted_debug_text(path))
                .field("source", source)
                .finish(),
        }
    }
}
