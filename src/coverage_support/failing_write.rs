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

/// Writer that always fails when bytes are written.
pub(super) struct FailingWrite;

impl Write for FailingWrite {
    /// Reports a synthetic write failure.
    ///
    /// # Parameters
    ///
    /// * `_buffer` - Bytes intentionally not written.
    ///
    /// # Returns
    ///
    /// Always returns an I/O error.
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("write failed"))
    }

    /// Flushes the synthetic writer.
    ///
    /// # Returns
    ///
    /// Always succeeds because the write path is tested separately.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
