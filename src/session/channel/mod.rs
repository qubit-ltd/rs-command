//! Bounded, runtime independent byte channels used by managed sessions.
mod reader;
mod state;
mod writer;

pub(crate) use reader::ChannelReader;
pub(crate) use state::{ByteChannel, CHANNEL_CAPACITY, MAX_BLOCK};
pub(crate) use writer::ChannelWriter;

/// The channel half-set shared by the process worker and session endpoints.
#[cfg(feature = "async")]
pub(crate) struct SessionChannels {
    pub(crate) bridge: std::sync::Arc<Bridge>,
    pub(crate) stdin: super::endpoints::CommandStdin,
    pub(crate) stdout: super::endpoints::CommandStdout,
    pub(crate) stderr: super::endpoints::CommandStderr,
    /// Owned by the process worker; dropping it means the child stopped reading.
    pub(crate) stdin_reader: ChannelReader,
    /// Owned by the process worker; dropping either writer publishes output EOF.
    pub(crate) stdout_writer: ChannelWriter,
    pub(crate) stderr_writer: ChannelWriter,
}

#[cfg(feature = "async")]
pub(crate) struct Bridge {
    input: std::sync::Arc<ByteChannel>,
    output: std::sync::Arc<ByteChannel>,
    error: std::sync::Arc<ByteChannel>,
}

#[cfg(feature = "async")]
impl SessionChannels {
    pub(crate) fn new() -> Self {
        let input = std::sync::Arc::new(ByteChannel::new());
        let output = std::sync::Arc::new(ByteChannel::new());
        let error = std::sync::Arc::new(ByteChannel::new());
        Self {
            bridge: std::sync::Arc::new(Bridge {
                input: input.clone(),
                output: output.clone(),
                error: error.clone(),
            }),
            stdin: super::endpoints::CommandStdin::new(input.clone()),
            stdout: super::endpoints::CommandStdout::new(output.clone()),
            stderr: super::endpoints::CommandStderr::new(error.clone()),
            stdin_reader: ChannelReader(input),
            stdout_writer: ChannelWriter(output),
            stderr_writer: ChannelWriter(error),
        }
    }
}

#[cfg(feature = "async")]
impl super::contracts::SessionBridge for Bridge {
    fn read_stdin(&self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.input.blocking_read(buffer)
    }
    fn write_stdout(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.output.blocking_write(bytes)
    }
    fn write_stderr(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.error.blocking_write(bytes)
    }
    fn close_stdin(&self) {
        self.input.close_writer();
    }
}
