/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::io::{
    self,
    Read,
};

/// Reader that always fails when read.
pub(super) struct FailingReader;

impl Read for FailingReader {
    /// Reports a synthetic read failure.
    ///
    /// # Parameters
    ///
    /// * `_buffer` - Destination buffer intentionally left untouched.
    ///
    /// # Returns
    ///
    /// Always returns an I/O error.
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("read failed"))
    }
}
