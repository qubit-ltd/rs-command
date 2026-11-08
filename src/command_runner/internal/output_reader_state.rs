// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::thread;

use super::captured_output::CapturedOutput;
use super::output_capture_error::OutputCaptureError;

/// Ownership state for one output reader's worker result.
#[derive(Debug)]
pub(super) enum OutputReaderState {
    /// The worker is running or has completed but its handle has not been
    /// joined.
    Running {
        /// Worker handle whose result has not yet been collected.
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
    },
    /// The joined worker result retained for polling or final collection.
    Finished {
        /// Captured output or the original read/tee failure.
        result: Result<CapturedOutput, OutputCaptureError>,
    },
}
