//! Session bridge and opaque endpoint state ports.
use std::io;
#[cfg(feature = "async")]
use std::task::{Context, Poll};

/// Output-channel behavior after the bounded per-stream buffer fills.
#[cfg(feature = "async")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SessionStreamPolicy {
    /// Apply backpressure until the caller consumes buffered output.
    #[default]
    Backpressure,
    /// Discard excess output and mark that stream incomplete while continuing to drain.
    Discard,
    /// Count stdout and stderr together, then signal termination after this many bytes.
    TerminateAfterBytes(u64),
}

/// Blocking byte bridge used by process workers.
pub trait SessionBridge: Send + Sync {
    fn read_stdin(&self, buffer: &mut [u8]) -> io::Result<usize>;
    fn write_stdout(&self, bytes: &[u8]) -> io::Result<()>;
    fn write_stderr(&self, bytes: &[u8]) -> io::Result<()>;
    fn close_stdin(&self);

    /// Reports whether stdout bytes were discarded before delivery.
    fn stdout_incomplete(&self) -> bool {
        false
    }

    /// Reports whether stderr bytes were discarded before delivery.
    fn stderr_incomplete(&self) -> bool {
        false
    }

    /// Reports whether the stdin consumer observed a broken pipe.
    fn stdin_broken_pipe(&self) -> bool {
        false
    }

    /// Records a child-side stdin BrokenPipe observed by the process writer.
    fn record_stdin_broken_pipe(&self) {}

    /// Reports whether the aggregate output byte budget requested termination.
    fn output_budget_exceeded(&self) -> bool {
        false
    }

    /// Returns the configured aggregate stdout and stderr observation limit.
    fn output_budget_limit(&self) -> Option<u64> {
        None
    }

    /// Returns the number of stdout and stderr bytes observed against the aggregate limit.
    fn output_bytes_observed(&self) -> u64 {
        0
    }
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
