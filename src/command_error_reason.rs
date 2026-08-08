// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Detailed primary failure reasons for command execution.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use qubit_clock::TimeError;

use crate::OutputStream;

/// Detailed primary reason carried by [`crate::CommandError`].
#[derive(Debug)]
#[non_exhaustive]
pub enum CommandErrorReason {
    /// The process could not be spawned.
    SpawnFailed {
        /// Operating-system spawn error.
        source: io::Error,
    },
    /// Waiting for process completion failed.
    WaitFailed {
        /// Operating-system wait error.
        source: io::Error,
    },
    /// Cancellation was requested before startup.
    CancelledBeforeStart,
    /// Termination after a timeout failed.
    KillFailed {
        /// Timeout that was exceeded.
        timeout: Duration,
        /// Process-tree termination error.
        process_tree_source: io::Error,
        /// Direct-child termination error.
        child_source: io::Error,
    },
    /// Reading a captured stream failed.
    ReadOutputFailed {
        /// Output stream whose reader failed.
        stream: OutputStream,
        /// Operating-system read error.
        source: io::Error,
    },
    /// Opening stdin failed.
    OpenInputFailed {
        /// Configured stdin path.
        path: PathBuf,
        /// Operating-system open error.
        source: io::Error,
    },
    /// Stdin path is not a regular file.
    NonRegularInputFile {
        /// Configured stdin path.
        path: PathBuf,
    },
    /// Opening an output tee failed.
    OpenOutputFailed {
        /// Output stream whose tee could not be opened.
        stream: OutputStream,
        /// Configured output path.
        path: PathBuf,
        /// Operating-system open error.
        source: io::Error,
    },
    /// Output tee path is not a regular file.
    NonRegularOutputFile {
        /// Output stream receiving the tee.
        stream: OutputStream,
        /// Configured output path.
        path: PathBuf,
    },
    /// Input and output files conflict.
    InputOutputConflict {
        /// Configured stdin path.
        input_path: PathBuf,
        /// Output stream whose path conflicts with stdin.
        output_stream: OutputStream,
        /// Conflicting output path.
        output_path: PathBuf,
    },
    /// Output tee files conflict.
    OutputFilesConflict {
        /// Configured stdout tee path.
        stdout_path: PathBuf,
        /// Configured stderr tee path.
        stderr_path: PathBuf,
    },
    /// An I/O file could not be inspected.
    InspectIoFileFailed {
        /// Configured path that could not be inspected.
        path: PathBuf,
        /// Operating-system inspection error.
        source: io::Error,
    },
    /// The stdin helper could not be started.
    StartInputThreadFailed {
        /// Thread creation error.
        source: io::Error,
    },
    /// An output helper could not be started.
    StartOutputThreadFailed {
        /// Output stream whose helper could not start.
        stream: OutputStream,
        /// Thread creation error.
        source: io::Error,
    },
    /// Clock or timer handling failed.
    TimeFailed {
        /// Clock or timer error.
        source: TimeError,
    },
    /// Writing configured stdin failed.
    WriteInputFailed {
        /// Operating-system write error.
        source: io::Error,
    },
    /// Writing a captured stream to a tee failed.
    WriteOutputFailed {
        /// Output stream whose tee write failed.
        stream: OutputStream,
        /// Configured output path.
        path: PathBuf,
        /// Operating-system write error.
        source: io::Error,
    },
    /// The command exceeded its timeout.
    TimedOut {
        /// Timeout that was exceeded.
        timeout: Duration,
    },
    /// The command was cancelled after startup.
    Cancelled,
    /// Process-tree cancellation failed.
    CancelFailed {
        /// Process-tree termination error.
        process_tree_source: io::Error,
        /// Direct-child termination error.
        child_source: io::Error,
    },
    /// Successful command output was truncated.
    OutputTruncated,
    /// The process exited with an unconfigured status.
    UnexpectedExit {
        /// Exit code reported by the process, when available.
        exit_code: Option<i32>,
        /// Exit codes configured as successful.
        expected: Vec<i32>,
    },
}
