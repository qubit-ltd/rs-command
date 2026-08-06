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

/// Reader that deterministically fails every read for coverage probes.
#[derive(Debug)]
pub(in super::super) struct FailingReader;

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
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("coverage reader failure"))
    }
}
