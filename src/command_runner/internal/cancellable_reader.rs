// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Child-pipe preparation for cancellation-aware output readers.

use std::{
    io,
    process::{
        ChildStderr,
        ChildStdout,
    },
};

/// A child output pipe that can be switched to cancellation-aware reads.
pub(in crate::command_runner) trait CancellableReader:
    io::Read + Send + 'static
{
    /// Prepares the underlying pipe for cancellation polling.
    fn prepare_for_cancellation(&self) -> io::Result<()>;
    /// Returns the underlying Unix descriptor for event-driven reads.
    #[cfg(unix)]
    fn raw_fd(&self) -> std::os::fd::RawFd;
}

impl CancellableReader for ChildStdout {
    /// Prepares stdout for cancellation-aware polling.
    fn prepare_for_cancellation(&self) -> io::Result<()> {
        prepare_pipe(self)
    }

    #[cfg(unix)]
    /// Returns stdout's underlying descriptor.
    fn raw_fd(&self) -> std::os::fd::RawFd {
        std::os::fd::AsRawFd::as_raw_fd(self)
    }
}

impl CancellableReader for ChildStderr {
    /// Prepares stderr for cancellation-aware polling.
    fn prepare_for_cancellation(&self) -> io::Result<()> {
        prepare_pipe(self)
    }

    #[cfg(unix)]
    /// Returns stderr's underlying descriptor.
    fn raw_fd(&self) -> std::os::fd::RawFd {
        std::os::fd::AsRawFd::as_raw_fd(self)
    }
}

/// Configures one Unix output pipe for non-blocking cancellation polling.
#[cfg(unix)]
fn prepare_pipe<T: std::os::fd::AsRawFd>(pipe: &T) -> io::Result<()> {
    // SAFETY: fcntl operates on the valid descriptor owned by `pipe`.
    unsafe {
        let flags = libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(
            pipe.as_raw_fd(),
            libc::F_SETFL,
            flags | libc::O_NONBLOCK,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
/// Leaves Windows output pipes unchanged because cancellation uses thread APIs.
fn prepare_pipe<T>(_pipe: &T) -> io::Result<()> {
    Ok(())
}
