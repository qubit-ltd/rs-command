use super::ByteChannel;
use std::sync::Arc;

pub(crate) struct ChannelWriter(pub(crate) Arc<ByteChannel>);

impl Drop for ChannelWriter {
    fn drop(&mut self) {
        self.0.close_writer();
    }
}
