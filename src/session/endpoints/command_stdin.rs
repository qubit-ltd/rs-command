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
use crate::session::contracts::StdinEndpointState;

/// Caller-facing input endpoint for a managed command session.
pub struct CommandStdin {
    /// Async I/O behavior delegated to the underlying bounded channel.
    pub(crate) state: Arc<dyn StdinEndpointState>,
    /// Channel whose endpoint lifecycle is tied to this value.
    channel: Arc<ByteChannel>,
}
impl CommandStdin {
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
impl Drop for CommandStdin {
    /// Closes or releases the corresponding channel side when dropped.
    fn drop(&mut self) {
        self.channel.close_writer();
    }
}

impl StdinEndpointState for ByteChannel {
    /// Forwards input writes to the bounded channel.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context for pending writes.
    /// * `input` - Input bytes offered to the child.
    ///
    /// # Safety
    ///
    /// The caller must uphold the async I/O trait contract for the supplied
    /// buffer.
    fn poll_write(&self, cx: &mut Context<'_>, input: &[u8]) -> Poll<io::Result<usize>> {
        ByteChannel::poll_write(self, cx, input)
    }
    /// Completes input writes without additional buffering.
    ///
    /// # Parameters
    ///
    /// * `_cx` - Async task context (unused by the in-memory channel). Forwards
    ///   the flush request to the endpoint state.
    fn poll_flush(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    /// Closes the input writer and wakes the child reader.
    ///
    /// # Parameters
    ///
    /// * `_cx` - Async task context (unused by the in-memory channel). Forwards
    ///   closure to the endpoint state.
    fn poll_close(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.close_writer();
        Poll::Ready(Ok(()))
    }
}

impl qubit_io::AsyncOutput for CommandStdin {
    /// Byte value transferred by this endpoint.
    type Item = u8;
    /// Reports that the endpoint buffers bytes in its bounded channel.
    fn is_buffered(&self) -> bool {
        true
    }
    /// Writes the requested input slice after validating its range.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used when the channel applies backpressure.
    /// * `input` - Source buffer containing input bytes.
    /// * `index` - Starting offset of the requested slice.
    /// * `count` - Number of bytes requested from the source buffer.
    ///
    /// # Safety
    ///
    /// The caller must uphold the async output trait requirements for the
    /// supplied range.
    unsafe fn poll_write_unchecked(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
        index: usize,
        count: usize,
    ) -> Poll<io::Result<usize>> {
        let Some(end) = index.checked_add(count) else {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidInput, "input range overflow")));
        };
        let Some(slice) = input.get(index..end) else {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input range is out of bounds",
            )));
        };
        self.as_mut().get_mut().state.poll_write(cx, slice)
    }
    /// Forwards the flush request to the endpoint state.
    ///
    /// # Parameters
    ///
    /// * `cx` - Async task context used by the endpoint state.
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().state.poll_flush(cx)
    }
}
impl qubit_io::AsyncClose for CommandStdin {
    /// Forwards closure to the endpoint state.
    ///
    /// # Parameters
    ///
    /// * `cx` - Async task context used by the endpoint state.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().state.poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Waker;

    use qubit_io::AsyncClose;
    use qubit_io::AsyncOutput;

    use super::CommandStdin;
    use crate::session::channel::ByteChannel;

    #[test]
    fn test_input_range_errors_and_close_rejects_later_writes() {
        let channel = Arc::new(ByteChannel::new());
        let mut endpoint = CommandStdin::new(Arc::clone(&channel));
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        let overflow =
            match unsafe { Pin::new(&mut endpoint).poll_write_unchecked(&mut context, b"input", usize::MAX, 1) } {
                std::task::Poll::Ready(result) => result,
                other => panic!("range overflow should be rejected immediately: {other:?}"),
            };
        assert_eq!(
            overflow.expect_err("overflow is invalid").kind(),
            io::ErrorKind::InvalidInput
        );

        let out_of_bounds = match unsafe { Pin::new(&mut endpoint).poll_write_unchecked(&mut context, b"input", 4, 2) }
        {
            std::task::Poll::Ready(result) => result,
            other => panic!("out-of-bounds input should be rejected immediately: {other:?}"),
        };
        assert_eq!(
            out_of_bounds.expect_err("range must fit within the input slice").kind(),
            io::ErrorKind::InvalidInput
        );

        assert!(endpoint.is_buffered());
        assert!(matches!(
            Pin::new(&mut endpoint).poll_flush(&mut context),
            std::task::Poll::Ready(Ok(()))
        ));
        assert!(matches!(
            unsafe { Pin::new(&mut endpoint).poll_write_unchecked(&mut context, b"abcdef", 2, 2) },
            std::task::Poll::Ready(Ok(2))
        ));
        let mut received = [0; 4];
        assert_eq!(channel.blocking_read(&mut received).unwrap(), 2);
        assert_eq!(&received[..2], b"cd");

        assert!(matches!(
            Pin::new(&mut endpoint).poll_close(&mut context),
            std::task::Poll::Ready(Ok(()))
        ));
        let write_after_close = endpoint.state.poll_write(&mut context, b"late");
        assert!(matches!(
            write_after_close,
            std::task::Poll::Ready(Err(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));
    }
}
