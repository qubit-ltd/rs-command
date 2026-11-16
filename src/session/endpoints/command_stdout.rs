// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use crate::session::channel::ByteChannel;
use crate::session::contracts::OutputEndpointState;
/// Caller-facing standard output endpoint for a managed command session.
pub struct CommandStdout {
    /// Async I/O behavior delegated to the underlying bounded channel.
    pub(crate) state: Arc<dyn OutputEndpointState>,
    /// Channel whose endpoint lifecycle is tied to this value.
    channel: Arc<ByteChannel>,
}
impl CommandStdout {
    /// Creates the endpoint around its shared bounded channel.
    ///
    /// # Parameters
    ///
    /// * `channel` - Shared byte channel owned by the session worker.
    pub(crate) fn new(channel: Arc<ByteChannel>) -> Self {
        Self {
            state: channel.clone(),
            channel,
        }
    }
}
impl Drop for CommandStdout {
    /// Closes or releases the corresponding channel side when dropped.
    fn drop(&mut self) {
        self.channel.drop_reader();
    }
}
impl OutputEndpointState for ByteChannel {
    /// Forwards output reads to the bounded channel.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context for pending reads.
    /// * `output` - Buffer receiving child output.
    fn poll_read(&self, cx: &mut Context<'_>, output: &mut [u8]) -> Poll<io::Result<usize>> {
        ByteChannel::poll_read(self, cx, output)
    }
}
impl qubit_io::AsyncInput for CommandStdout {
    /// Byte value transferred by this endpoint.
    type Item = u8;
    /// Reports that the endpoint buffers bytes in its bounded channel.
    fn is_buffered(&self) -> bool {
        true
    }
    /// Reads output into the requested buffer range after validating it.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used when no output is ready.
    /// * `output` - Destination buffer for child output.
    /// * `index` - Starting offset of the destination range.
    /// * `count` - Number of bytes requested in the destination range.
    ///
    /// # Safety
    ///
    /// The caller must uphold the async input trait requirements for the
    /// supplied range.
    unsafe fn poll_read_unchecked(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<io::Result<usize>> {
        let Some(end) = index.checked_add(count) else {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output range overflow",
            )));
        };
        let Some(slice) = output.get_mut(index..end) else {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output range is out of bounds",
            )));
        };
        self.as_mut().get_mut().state.poll_read(cx, slice)
    }
}
