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
    /// Combines an optional writer and diagnostic path without silently
    /// dropping either half of a configured tee.
    ///
    /// # Parameters
    ///
    /// * `writer` - Optional destination for streamed output bytes.
    /// * `path` - Optional path used to identify the destination in
    ///   diagnostics.
    ///
    /// # Returns
    ///
    /// `Some` when both parts are present, or `None` when neither is present.
    ///
    /// # Panics
    ///
    /// Panics when exactly one of `writer` and `path` is present.
    #[inline]
    pub(in crate::command_runner) fn from_parts(
        writer: Option<Box<dyn Write + Send>>,
        path: Option<PathBuf>,
    ) -> Option<Self> {
        match (writer, path) {
            (Some(writer), Some(path)) => Some(Self::new(writer, path)),
            (None, None) => None,
            _ => panic!("output tee writer and diagnostic path must be configured together"),
        }
    }

    /// Creates a streaming destination with its diagnostic path.
    ///
    /// # Parameters
    ///
    /// * `writer` - Destination receiving every emitted output byte.
    /// * `path` - Path identifying the destination in write diagnostics.
    ///
    /// # Returns
    ///
    /// A destination that pairs the writer with its diagnostic path.
    #[must_use]
    #[inline]
    pub(in crate::command_runner) fn new(writer: Box<dyn Write + Send>, path: PathBuf) -> Self {
        Self { writer, path }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::OutputTee;

    #[test]
    #[should_panic(expected = "output tee writer and diagnostic path must be configured together")]
    fn test_output_tee_rejects_unpaired_parts() {
        let _ = OutputTee::from_parts(None, Some(PathBuf::from("stdout.log")));
    }

    #[test]
    #[should_panic(expected = "output tee writer and diagnostic path must be configured together")]
    fn test_output_tee_rejects_writer_without_path() {
        let _ = OutputTee::from_parts(Some(Box::new(Vec::<u8>::new())), None);
    }
}
