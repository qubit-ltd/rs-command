// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io::{
    self,
    Read,
};

/// Reader that returns one prefix and then fails for coverage probes.
#[derive(Debug)]
pub(in super::super) struct FailingReader {
    /// Bytes returned before the injected failure.
    prefix: Vec<u8>,
    /// Whether the prefix has already been returned.
    returned: bool,
}

impl FailingReader {
    /// Creates a reader that returns `prefix` once before failing.
    pub(in super::super) fn with_prefix(prefix: Vec<u8>) -> Self {
        Self {
            prefix,
            returned: false,
        }
    }
}

impl Read for FailingReader {
    /// Returns the injected read failure.
    ///
    /// # Parameters
    ///
    /// * `_buffer` - Unused read destination.
    ///
    /// # Errors
    ///
    /// Always returns the injected I/O error.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.returned {
            self.returned = true;
            buffer[..self.prefix.len()].copy_from_slice(&self.prefix);
            return Ok(self.prefix.len());
        }
        Err(io::Error::other("coverage reader failure"))
    }
}
