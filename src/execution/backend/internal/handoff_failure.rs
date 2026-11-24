// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

use super::HandoffFailureKind;
use super::ReaperOwner;

/// Describes a failed reaper handoff and returns cleanup ownership to its
/// caller.
pub(crate) struct HandoffFailure {
    /// Classifies whether the queue was full or unavailable.
    pub(crate) kind: HandoffFailureKind,
    /// Cleanup responsibility returned to the caller for fallback handling.
    pub(crate) owner: ReaperOwner,
    /// Optional cause from pool startup. Debug output reports its OS error
    /// code.
    pub(crate) source: Option<io::Error>,
}

/// Provides diagnostic formatting for a reaper handoff failure.
impl std::fmt::Debug for HandoffFailure {
    /// Writes the failure kind and optional startup OS error code.
    ///
    /// # Parameters
    /// * `formatter`: Destination for the formatted debug representation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandoffFailure")
            .field("kind", &self.kind)
            .field("raw_os_error", &self.source.as_ref().and_then(io::Error::raw_os_error))
            .finish_non_exhaustive()
    }
}

impl HandoffFailure {
    /// Logs the failure kind and optional OS error code for the cleanup
    /// context.
    ///
    /// # Parameters
    /// * `context`: Identifies the cleanup path that encountered the failure.
    pub(crate) fn log_unavailable(&self, context: &str) {
        let os_error = self.source.as_ref().and_then(io::Error::raw_os_error);
        log::error!(
            "{context}: process reaper unavailable ({:?}, os_error={os_error:?})",
            self.kind
        );
    }
}
