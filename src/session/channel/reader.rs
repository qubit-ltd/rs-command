use super::ByteChannel;
use std::sync::Arc;

pub(crate) struct ChannelReader(pub(crate) Arc<ByteChannel>);

impl Drop for ChannelReader {
    fn drop(&mut self) {
        self.0.drop_reader();
    }
}
