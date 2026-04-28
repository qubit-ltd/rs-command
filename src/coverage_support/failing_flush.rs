/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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
