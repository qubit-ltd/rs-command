// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Session bridge and opaque endpoint state ports.
use std::io;
#[cfg(feature = "async")]
use std::task::Context;
#[cfg(feature = "async")]
use std::task::Poll;

/// Output-channel behavior after the bounded per-stream buffer fills.
#[cfg(feature = "async")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SessionStreamPolicy {
    /// Apply backpressure until the caller consumes buffered output.
    #[default]
    Backpressure,
    /// Discard excess output and mark that stream incomplete while continuing
    /// to drain.
    Discard,
    /// Count stdout and stderr together, then signal termination after this
    /// many bytes.
    TerminateAfterBytes(
        /// Aggregate output limit that triggers termination.
        u64,
    ),
}

/// Blocking byte bridge used by process workers.
pub trait SessionBridge: Send + Sync {
    /// Reads child input from the caller-facing stdin stream.
    ///
    /// # Parameters
    ///
    /// * `buffer` - Destination for the next available input bytes.
    fn read_stdin(&self, buffer: &mut [u8]) -> io::Result<usize>;
    /// Publishes child standard output to the caller-facing stream.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Output bytes produced by the child.
    fn write_stdout(&self, bytes: &[u8]) -> io::Result<()>;
    /// Publishes child standard error to the caller-facing stream.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Error bytes produced by the child.
    fn write_stderr(&self, bytes: &[u8]) -> io::Result<()>;
    /// Closes the child-facing stdin stream after input is complete.
    ///
    /// # Parameters
    ///
    /// This operation has no parameters.
    fn close_stdin(&self);

    /// Stops accepting future stdout and stderr bytes while preserving buffered
    /// output.
    fn request_output_stop(&self) {}

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

    /// Returns the number of stdout and stderr bytes observed against the
    /// aggregate limit.
    fn output_bytes_observed(&self) -> u64 {
        0
    }
}

/// State contract delegated to by the async stdin endpoint.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub(crate) trait StdinEndpointState: Send + Sync {
    /// Polls for capacity to write input from the caller.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used to register for available capacity.
    /// * `input` - Input bytes offered to the child.
    fn poll_write(&self, cx: &mut Context<'_>, input: &[u8]) -> Poll<io::Result<usize>>;
    /// Polls completion of pending input writes.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used by the async input contract.
    fn poll_flush(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
    /// Polls closure of the child-facing input stream.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used by the async input contract.
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

/// State contract delegated to by async stdout/stderr endpoints.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub(crate) trait OutputEndpointState: Send + Sync {
    /// Polls for output bytes from the child.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used to register when output is not ready.
    /// * `output` - Buffer receiving available child output.
    fn poll_read(&self, cx: &mut Context<'_>, output: &mut [u8]) -> Poll<io::Result<usize>>;
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::SessionBridge;

    struct DefaultBridge;

    impl SessionBridge for DefaultBridge {
        fn read_stdin(&self, _buffer: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }

        fn write_stdout(&self, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }

        fn write_stderr(&self, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }

        fn close_stdin(&self) {}
    }

    #[test]
    fn test_session_bridge_defaults_report_no_stream_failures_or_budget() {
        let bridge = DefaultBridge;

        bridge.request_output_stop();
        bridge.record_stdin_broken_pipe();
        assert!(!bridge.stdout_incomplete());
        assert!(!bridge.stderr_incomplete());
        assert!(!bridge.stdin_broken_pipe());
        assert!(!bridge.output_budget_exceeded());
        assert_eq!(bridge.output_budget_limit(), None);
        assert_eq!(bridge.output_bytes_observed(), 0);
    }
}
