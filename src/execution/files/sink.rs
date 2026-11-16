use crate::execution::contracts::PreparedSink;
use std::io::{self, Write};

/// Writes tee bytes while serializing helpers that share an owned sink.
pub(crate) fn write_sink(sink: &PreparedSink, bytes: &[u8]) -> io::Result<()> {
    sink.file
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?
        .write_all(bytes)
}

/// Flushes a shared sink after its final helper write.
pub(crate) fn flush_sink(sink: &PreparedSink) -> io::Result<()> {
    sink.file
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?
        .flush()
}
