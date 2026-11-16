//! Session bridge and opaque endpoint state ports.
use std::io;
#[cfg(feature = "async")]
use std::task::{Context, Poll};

/// Blocking byte bridge used by process workers.
pub trait SessionBridge: Send + Sync {
    fn read_stdin(&self, buffer: &mut [u8]) -> io::Result<usize>;
    fn write_stdout(&self, bytes: &[u8]) -> io::Result<()>;
    fn write_stderr(&self, bytes: &[u8]) -> io::Result<()>;
    fn close_stdin(&self);
}

/// State contract delegated to by the async stdin endpoint.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub(crate) trait StdinEndpointState: Send + Sync {
    fn poll_write(&self, cx: &mut Context<'_>, input: &[u8]) -> Poll<io::Result<usize>>;
    fn poll_flush(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

/// State contract delegated to by async stdout/stderr endpoints.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub(crate) trait OutputEndpointState: Send + Sync {
    fn poll_read(&self, cx: &mut Context<'_>, output: &mut [u8]) -> Poll<io::Result<usize>>;
}
