// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Child-pipe preparation for cancellation-aware output readers.

use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::fd::RawFd;
use std::process::ChildStderr;
use std::process::ChildStdout;

#[cfg(unix)]
use libc::F_GETFL;
#[cfg(unix)]
use libc::F_SETFL;
#[cfg(unix)]
use libc::O_NONBLOCK;
#[cfg(unix)]
use libc::fcntl;

/// A child output pipe that can be switched to cancellation-aware reads.
pub(in crate::command_runner) trait CancellableReader: io::Read + Send + 'static {
    /// Prepares the underlying pipe for cancellation polling.
    fn prepare_for_cancellation(&self) -> io::Result<()>;
    /// Returns the underlying Unix descriptor for event-driven reads.
    #[cfg(unix)]
    fn raw_fd(&self) -> RawFd;
}

impl CancellableReader for ChildStdout {
    /// Prepares stdout for cancellation-aware polling.
    fn prepare_for_cancellation(&self) -> io::Result<()> {
        prepare_pipe(self)
    }

    /// Returns stdout's underlying descriptor.
    #[cfg(unix)]
    fn raw_fd(&self) -> RawFd {
        self.as_raw_fd()
    }
}

impl CancellableReader for ChildStderr {
    /// Prepares stderr for cancellation-aware polling.
    fn prepare_for_cancellation(&self) -> io::Result<()> {
        prepare_pipe(self)
    }

    /// Returns stderr's underlying descriptor.
    #[cfg(unix)]
    fn raw_fd(&self) -> RawFd {
        self.as_raw_fd()
    }
}

/// Configures one Unix output pipe for non-blocking cancellation polling.
#[cfg(unix)]
fn prepare_pipe<T: AsRawFd>(pipe: &T) -> io::Result<()> {
    // SAFETY: fcntl operates on the valid descriptor owned by `pipe`.
    unsafe {
        let flags = fcntl(pipe.as_raw_fd(), F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if fcntl(pipe.as_raw_fd(), F_SETFL, flags | O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Leaves Windows output pipes unchanged because cancellation uses thread APIs.
#[cfg(windows)]
fn prepare_pipe<T>(_pipe: &T) -> io::Result<()> {
    Ok(())
}
