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
/// Caller-facing standard error endpoint for a managed command session.
pub struct CommandStderr {
    /// Async I/O behavior delegated to the underlying bounded channel.
    pub(crate) state: Arc<dyn OutputEndpointState>,
    /// Channel whose endpoint lifecycle is tied to this value.
    channel: Arc<ByteChannel>,
}
impl CommandStderr {
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
impl Drop for CommandStderr {
    /// Closes or releases the corresponding channel side when dropped.
    fn drop(&mut self) {
        self.channel.drop_reader();
    }
}
impl qubit_io::AsyncInput for CommandStderr {
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Waker;

    use qubit_io::AsyncInput;

    use super::CommandStderr;
    use crate::session::channel::ByteChannel;

    #[test]
    fn test_stderr_range_errors_and_drop_marks_reader_gone() {
        let channel = Arc::new(ByteChannel::new());
        let mut endpoint = CommandStderr::new(Arc::clone(&channel));
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut output = [0; 4];

        let overflow = match unsafe {
            Pin::new(&mut endpoint).poll_read_unchecked(&mut context, &mut output, usize::MAX, 1)
        } {
            std::task::Poll::Ready(result) => result,
            other => panic!("range overflow should be rejected immediately: {other:?}"),
        };
        assert_eq!(overflow.expect_err("overflow is invalid").kind(), io::ErrorKind::InvalidInput);

        let out_of_bounds = match unsafe {
            Pin::new(&mut endpoint).poll_read_unchecked(&mut context, &mut output, 3, 2)
        } {
            std::task::Poll::Ready(result) => result,
            other => panic!("out-of-bounds output should be rejected immediately: {other:?}"),
        };
        assert_eq!(
            out_of_bounds
                .expect_err("range must fit within the output slice")
                .kind(),
            io::ErrorKind::InvalidInput
        );

        assert!(endpoint.is_buffered());
        channel.blocking_write(b"xy").expect("stderr bytes should be queued");
        assert!(matches!(
            unsafe { Pin::new(&mut endpoint).poll_read_unchecked(&mut context, &mut output, 1, 2) },
            std::task::Poll::Ready(Ok(2))
        ));
        assert_eq!(output, [0, b'x', b'y', 0]);

        channel.blocking_write(b"queued").expect("error output should queue before endpoint drop");
        drop(endpoint);
        assert!(channel.incomplete());
        assert_eq!(channel.blocking_read(&mut [0; 8]).unwrap(), 0);
        assert!(matches!(
            channel.poll_write(&mut context, b"late"),
            std::task::Poll::Ready(Err(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));
        assert!(channel.broken_pipe());
    }
}
