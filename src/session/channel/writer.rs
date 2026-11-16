use std::sync::Arc;

use super::ByteChannel;

pub(crate) struct ChannelWriter(pub(crate) Arc<ByteChannel>);

impl Drop for ChannelWriter {
    fn drop(&mut self) {
        self.0.close_writer();
    }
}
