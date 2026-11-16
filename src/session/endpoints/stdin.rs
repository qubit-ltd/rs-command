use crate::session::{channel::ByteChannel, contracts::StdinEndpointState};
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub struct CommandStdin {
    pub(crate) state: Arc<dyn StdinEndpointState>,
    channel: Arc<ByteChannel>,
}
impl CommandStdin {
    pub(crate) fn new(channel: Arc<ByteChannel>) -> Self {
        Self {
            state: channel.clone(),
            channel,
        }
    }
}
impl Drop for CommandStdin {
    fn drop(&mut self) {
        self.channel.close_writer();
    }
}

impl StdinEndpointState for ByteChannel {
    fn poll_write(&self, cx: &mut Context<'_>, input: &[u8]) -> Poll<io::Result<usize>> {
        ByteChannel::poll_write(self, cx, input)
    }
    fn poll_flush(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.close_writer();
        Poll::Ready(Ok(()))
    }
}

impl qubit_io::AsyncOutput for CommandStdin {
    type Item = u8;
    fn is_buffered(&self) -> bool {
        true
    }
    unsafe fn poll_write_unchecked(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
        index: usize,
        count: usize,
    ) -> Poll<io::Result<usize>> {
        let Some(end) = index.checked_add(count) else {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input range overflow",
            )));
        };
        let Some(slice) = input.get(index..end) else {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input range is out of bounds",
            )));
        };
        self.as_mut().get_mut().state.poll_write(cx, slice)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().state.poll_flush(cx)
    }
}
impl qubit_io::AsyncClose for CommandStdin {
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().state.poll_close(cx)
    }
}
