// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Helper-thread side of cancellation notification.

#[cfg(unix)]
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Receives cancellation notifications from the owning command runner.
#[derive(Debug)]
pub(in crate::command_runner) struct IoCancellationToken {
    /// Shared cancellation state written by the owner.
    pub(in crate::command_runner) cancelled: Arc<AtomicBool>,
    /// Unix socket read end used to wake the poll operation.
    #[cfg(unix)]
    pub(in crate::command_runner) wakeup: std::os::unix::net::UnixStream,
}

impl IoCancellationToken {
    /// Returns whether cancellation has been requested.
    #[must_use]
    pub(in crate::command_runner) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Waits until a child-pipe descriptor is ready or cancellation is
    /// requested.
    ///
    /// # Parameters
    ///
    /// * `fd` - Child pipe descriptor to observe.
    /// * `events` - `poll` event mask for the child descriptor.
    ///
    /// # Returns
    ///
    /// `true` when the child descriptor is ready; `false` when cancellation
    /// was requested or its wakeup descriptor became readable.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error when `poll` fails for a reason other
    /// than interruption.
    #[cfg(unix)]
    pub(in crate::command_runner) fn wait_for_fd(
        &self,
        fd: std::os::fd::RawFd,
        events: libc::c_short,
    ) -> io::Result<bool> {
        use std::os::fd::AsRawFd;

        loop {
            if self.is_cancelled() {
                return Ok(false);
            }
            let mut descriptors = [
                libc::pollfd { fd, events, revents: 0 },
                libc::pollfd {
                    fd: self.wakeup.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: both descriptors are owned by self or the caller and the
            // array remains valid for the duration of this blocking call.
            let result = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, -1) };
            if result < 0 {
                let source = io::Error::last_os_error();
                if source.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(source);
            }
            if self.is_cancelled() || descriptors[1].revents != 0 {
                return Ok(false);
            }
            if descriptors[0].revents != 0 {
                return Ok(true);
            }
        }
    }
}
