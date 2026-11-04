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
#[cfg(all(test, windows))]
use std::sync::mpsc::Sender;
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
    /// Deterministic raw cancellation failure used by Windows unit tests.
    #[cfg(test)]
    test_raw_os_error: Option<i32>,
    /// Reports the native cancellation result to a Windows probe.
    #[cfg(all(test, windows))]
    test_observer: Option<Sender<Option<i32>>>,
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
                    #[cfg(test)]
                    test_raw_os_error: None,
                    #[cfg(all(test, windows))]
                    test_observer: None,
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
                    #[cfg(test)]
                    test_raw_os_error: None,
                    #[cfg(all(test, windows))]
                    test_observer: None,
                },
                IoCancellationToken { cancelled },
            ))
        }
    }

    /// Creates cancellation state that reports a deterministic test failure.
    #[cfg(test)]
    /// # Parameters
    pub(super) fn failing(message: &'static str) -> Self {
        let (mut cancellation, _token) = Self::pair().expect("test cancellation pair should be created");
        cancellation.test_failure = Some((io::ErrorKind::Other, message));
        cancellation
    }

    /// Creates cancellation state that reports one raw OS error.
    #[cfg(all(test, windows))]
    /// # Parameters
    pub(super) fn failing_raw_os_error(raw_os_error: i32) -> Self {
        let (mut cancellation, _token) = Self::pair().expect("test cancellation pair should be created");
        cancellation.test_raw_os_error = Some(raw_os_error);
        cancellation
    }

    /// Marks the operation cancelled and wakes the helper thread.
    ///
    /// # Parameters
    ///
    /// * `join` - Helper thread whose blocking I/O may need interruption.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the Unix wakeup write fails for a reason other
    /// than a broken pipe, or if interrupting the helper thread fails.
    /// # Type Parameters
    ///
    /// * `T` - Type of helper thread result.
    pub(in crate::command_runner) fn cancel<T>(&self, join: &JoinHandle<T>) -> io::Result<()> {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(test)]
        if let Some(raw_os_error) = self.test_raw_os_error {
            let result = Err(io::Error::from_raw_os_error(raw_os_error));
            #[cfg(windows)]
            self.observe_test_result(&result);
            return result;
        }
        #[cfg(test)]
        if let Some((kind, message)) = self.test_failure {
            let result = Err(io::Error::new(kind, message));
            #[cfg(windows)]
            self.observe_test_result(&result);
            return result;
        }
        let result = {
            #[cfg(unix)]
            {
                use std::io::Write;

                if let Err(error) = (&self.notifier).write_all(&[1])
                    && error.kind() != io::ErrorKind::BrokenPipe
                {
                    return Err(error);
                }
                let _ = join;
                Ok(())
            }
            #[cfg(windows)]
            {
                super::cancel::cancel_synchronous_io(join)
            }
        };
        #[cfg(all(test, windows))]
        self.observe_test_result(&result);
        result
    }

    #[cfg(all(test, windows))]
    pub(super) fn observe_test_cancellation(&mut self, observer: Sender<Option<i32>>) {
        self.test_observer = Some(observer);
    }

    #[cfg(all(test, windows))]
    fn observe_test_result(&self, result: &io::Result<()>) {
        if let Some(observer) = &self.test_observer {
            let _ = observer.send(result.as_ref().err().and_then(io::Error::raw_os_error));
        }
    }
}
