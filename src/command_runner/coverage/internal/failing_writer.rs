// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io::{
    self,
    Write,
};

/// Writer that deterministically fails either writes or flushes for coverage
/// probes.
#[derive(Debug)]
pub(in super::super) struct FailingWriter {
    /// Whether writes fail instead of succeeding before the injected flush
    /// failure.
    pub(in super::super) fail_write: bool,
}

impl Write for FailingWriter {
    /// Completes the write or returns the configured injected failure.
    ///
    /// # Parameters
    ///
    /// * `buffer` - Bytes the probe attempts to write.
    ///
    /// # Returns
    ///
    /// Written byte count when write failures are disabled.
    ///
    /// # Errors
    ///
    /// Returns the injected I/O error when write failures are enabled.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.fail_write {
            Err(io::Error::other("coverage writer failure"))
        } else {
            Ok(buffer.len())
        }
    }

    /// Completes the flush or returns the configured injected failure.
    ///
    /// # Errors
    ///
    /// Returns the injected I/O error when write failures are disabled.
    fn flush(&mut self) -> io::Result<()> {
        if self.fail_write {
            Ok(())
        } else {
            Err(io::Error::other("coverage flush failure"))
        }
    }
}
