// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::output_tee::OutputTee;

/// Output capture options moved into a reader thread.
pub(in crate::command_runner) struct OutputCaptureOptions {
    /// Maximum bytes retained in memory.
    pub(in crate::command_runner) max_bytes: Option<usize>,
    /// Optional writer receiving a streaming copy.
    pub(in crate::command_runner) tee: Option<OutputTee>,
}

impl OutputCaptureOptions {
    /// Creates output capture options.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Optional in-memory retention limit.
    /// * `tee` - Optional writer receiving all emitted bytes.
    ///
    /// # Returns
    ///
    /// Capture options moved into the output reader thread.
    #[inline]
    pub(in crate::command_runner) fn new(
        max_bytes: Option<usize>,
        tee: Option<OutputTee>,
    ) -> Self {
        Self { max_bytes, tee }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::OutputCaptureOptions;
    use super::OutputTee;

    #[test]
    fn new_accepts_a_writer_and_diagnostic_path() {
        let tee = OutputTee::new(Box::new(Vec::<u8>::new()), PathBuf::from("stdout.log"));
        let options = OutputCaptureOptions::new(Some(4), Some(tee));
        assert_eq!(options.max_bytes, Some(4));
        assert!(options.tee.is_some());
    }
}
