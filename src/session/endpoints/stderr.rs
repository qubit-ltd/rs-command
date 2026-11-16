use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use crate::session::channel::ByteChannel;
use crate::session::contracts::OutputEndpointState;
pub struct CommandStderr {
    pub(crate) state: Arc<dyn OutputEndpointState>,
    channel: Arc<ByteChannel>,
}
impl CommandStderr {
    pub(crate) fn new(channel: Arc<ByteChannel>) -> Self {
        Self {
            state: channel.clone(),
            channel,
        }
    }
}
impl Drop for CommandStderr {
    fn drop(&mut self) {
        self.channel.drop_reader();
    }
}
impl qubit_io::AsyncInput for CommandStderr {
    type Item = u8;
    fn is_buffered(&self) -> bool {
        true
    }
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
