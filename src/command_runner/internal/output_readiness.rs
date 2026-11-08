// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;

use super::io_cancellation_token::IoCancellationToken;

/// Readiness observation used to bound the next budgeted pipe read.
/// No output budget is reserved by this observation.
pub(in crate::command_runner) enum OutputReadiness {
    /// Allows a read capped by the supplied byte capacity, the destination
    /// buffer, and the remaining total output budget. On Unix the capacity
    /// is the 8 KiB read-chunk cap after poll reports readiness, not an exact
    /// available-byte count. On Windows it is the count observed by
    /// PeekNamedPipe at the time of the probe.
    Readable(
        /// Maximum byte capacity supplied by the readiness observation.
        usize,
    ),
    /// Indicates that the Windows pipe probe observed the pipe closed.
    #[cfg(windows)]
    Eof,
    /// Indicates helper cancellation; this does not establish stream EOF.
    Cancelled,
}

/// Native descriptor used to poll a Unix output pipe.
#[cfg(unix)]
pub(super) type OutputFd = std::os::fd::RawFd;
/// Native handle used to inspect Windows pipe availability.
#[cfg(windows)]
pub(super) type OutputFd = std::os::windows::io::RawHandle;

/// Waits for Unix output-pipe readiness or cancellation.
///
/// # Parameters
///
/// * `fd` - Output descriptor to poll.
/// * `cancellation` - Token that can stop the readiness wait.
#[cfg(unix)]
pub(super) fn wait_output_readiness(
    fd: OutputFd,
    cancellation: &IoCancellationToken,
) -> io::Result<OutputReadiness> {
    if cancellation.wait_for_fd(fd, libc::POLLIN)? {
        Ok(OutputReadiness::Readable(8 * 1024))
    } else {
        Ok(OutputReadiness::Cancelled)
    }
}

/// Waits for Windows output-pipe data or cancellation.
///
/// # Parameters
///
/// * `fd` - Output pipe handle to inspect.
/// * `cancellation` - Token that can stop the readiness wait.
#[cfg(windows)]
pub(super) fn wait_output_readiness(
    fd: OutputFd,
    cancellation: &IoCancellationToken,
) -> io::Result<OutputReadiness> {
    use std::ffi::c_void;
    unsafe extern "system" {
        /// Observes bytes available in the named pipe without consuming them.
        ///
        /// # Parameters
        ///
        /// * `handle` - Named-pipe handle to inspect.
        /// * `buffer` - Optional destination for bytes; this probe passes null.
        /// * `size` - Capacity of `buffer`; this probe passes zero.
        /// * `read` - Optional pointer receiving the bytes copied.
        /// * `available` - Pointer receiving the currently available byte
        ///   count.
        /// * `left` - Optional pointer receiving the remaining message byte
        ///   count.
        fn PeekNamedPipe(
            handle: OutputFd,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }
    let mut poll_count = 0usize;
    loop {
        if cancellation.is_cancelled() {
            return Ok(OutputReadiness::Cancelled);
        }
        let mut available = 0u32;
        // SAFETY: the handle is owned by the output reader and outlives this wait.
        let result = unsafe {
            PeekNamedPipe(
                fd,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            let source = io::Error::last_os_error();
            if source.kind() == io::ErrorKind::BrokenPipe {
                return Ok(OutputReadiness::Eof);
            }
            return Err(source);
        }
        if available > 0 {
            return Ok(OutputReadiness::Readable(available as usize));
        }
        std::thread::sleep(super::wait_policy::next_poll_interval(poll_count));
        poll_count = poll_count.saturating_add(1);
    }
}
