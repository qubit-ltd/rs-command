// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cancellation notification for helper-thread pipe I/O.

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use super::io_cancellation_token::IoCancellationToken;

/// Sends cancellation notifications to one helper thread.
#[derive(Debug)]
pub(in crate::command_runner) struct IoCancellation {
    /// Shared cancellation state observed by the helper.
    cancelled: Arc<AtomicBool>,
    /// Unix socket used to wake a blocked poll call.
    #[cfg(unix)]
    notifier: std::os::unix::net::UnixStream,
    /// Deterministic cancellation failure used by private unit tests.
    #[cfg(test)]
    test_failure: Option<(io::ErrorKind, &'static str)>,
}

impl IoCancellation {
    /// Creates a cancellation sender and its corresponding helper token.
    ///
    /// # Returns
    ///
    /// A sender/token pair, or an I/O error when the Unix wakeup channel cannot
    /// be created or configured.
    pub(in crate::command_runner) fn pair() -> io::Result<(Self, IoCancellationToken)> {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;

            let (notifier, wakeup) = UnixStream::pair()?;
            notifier.set_nonblocking(true)?;
            wakeup.set_nonblocking(true)?;
            let cancelled = Arc::new(AtomicBool::new(false));
            Ok((
                Self {
                    cancelled: Arc::clone(&cancelled),
                    notifier,
                    #[cfg(test)]
                    test_failure: None,
                },
                IoCancellationToken { cancelled, wakeup },
            ))
        }

        #[cfg(windows)]
        {
            let cancelled = Arc::new(AtomicBool::new(false));
            Ok((
                Self {
                    cancelled: Arc::clone(&cancelled),
                    #[cfg(test)]
                    test_failure: None,
                },
                IoCancellationToken { cancelled },
            ))
        }
    }

    /// Marks the operation cancelled and wakes the helper thread.
    ///
    /// # Parameters
    ///
    /// * `join` - Helper thread whose blocking I/O may need interruption.
    pub(in crate::command_runner) fn cancel<T>(&self, join: &JoinHandle<T>) -> io::Result<()> {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(test)]
        if let Some((kind, message)) = self.test_failure {
            return Err(io::Error::new(kind, message));
        }
        #[cfg(unix)]
        {
            use std::io::Write;

            (&self.notifier).write_all(&[1])?;
            let _ = join;
            Ok(())
        }
        #[cfg(windows)]
        {
            super::cancel::cancel_synchronous_io(join)
        }
    }

    /// Creates cancellation state that reports a deterministic test failure.
    #[cfg(test)]
    pub(super) fn failing(message: &'static str) -> Self {
        let (mut cancellation, _token) =
            Self::pair().expect("test cancellation pair should be created");
        cancellation.test_failure = Some((io::ErrorKind::Other, message));
        cancellation
    }
}
