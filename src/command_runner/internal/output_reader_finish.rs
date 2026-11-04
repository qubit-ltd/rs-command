// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::captured_output::CapturedOutput;
use super::output_capture_error::OutputCaptureError;

/// Result of finishing an output reader after requesting cancellation.
pub(super) enum OutputReaderFinish {
    /// The helper was joined; its result is either captured output or a
    /// capture error.
    Joined(
        /// Captured output or the error returned by the joined helper.
        Result<CapturedOutput, OutputCaptureError>,
    ),
    /// The helper was not joined because cancellation failed and completion
    /// was not observed before the shared confirmation deadline. The output
    /// is a point-in-time snapshot and the helper may still be running.
    Detached(
        /// Point-in-time output snapshot from a helper that was not joined.
        CapturedOutput,
    ),
}

impl OutputReaderFinish {
    /// Converts the finish state to the output result used by collection.
    ///
    /// A detached snapshot is returned as `Ok`, but does not indicate that
    /// collection completed successfully: its `complete` flag is false and
    /// its bytes may be incomplete. Cancellation errors are retained
    /// separately by the cleanup path.
    pub(super) fn into_output_result(self) -> Result<CapturedOutput, OutputCaptureError> {
        match self {
            Self::Joined(result) => result,
            Self::Detached(output) => Ok(output),
        }
    }
}
