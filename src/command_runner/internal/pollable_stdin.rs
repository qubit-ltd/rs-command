// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cancellation-aware writable stdin abstraction.

use std::io;
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(unix)]
use libc::POLLOUT;

use super::io_cancellation_token::IoCancellationToken;

/// Stdin writer abstraction that can wait for writable capacity.
pub(super) trait PollableStdin: Write {
    /// Waits for the pipe to accept another write or observes cancellation.
    fn wait_writable(&self, cancellation: &IoCancellationToken) -> io::Result<bool>;
}

#[cfg(unix)]
impl<T: Write + AsRawFd> PollableStdin for T {
    /// Waits for the Unix descriptor to become writable or cancellation.
    #[inline]
    fn wait_writable(&self, cancellation: &IoCancellationToken) -> io::Result<bool> {
        cancellation.wait_for_fd(self.as_raw_fd(), POLLOUT)
    }
}

#[cfg(windows)]
impl<T: Write> PollableStdin for T {
    /// Checks cancellation before allowing the next write on Windows.
    #[inline]
    fn wait_writable(&self, cancellation: &IoCancellationToken) -> io::Result<bool> {
        Ok(!cancellation.is_cancelled())
    }
}
