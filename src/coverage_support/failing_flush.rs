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
    Write,
};

/// Writer that accepts bytes but fails when flushed.
pub(super) struct FailingFlush;

impl Write for FailingFlush {
    /// Pretends all bytes were written successfully.
    ///
    /// # Parameters
    ///
    /// * `buffer` - Bytes accepted by the synthetic writer.
    ///
    /// # Returns
    ///
    /// Number of bytes accepted.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        Ok(buffer.len())
    }

    /// Reports a synthetic flush failure.
    ///
    /// # Returns
    ///
    /// Always returns an I/O error.
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("flush failed"))
    }
}
