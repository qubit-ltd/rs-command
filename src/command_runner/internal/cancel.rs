// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Platform support for interrupting synchronous helper-thread I/O.

use std::io;
use std::thread::JoinHandle;

/// Windows error returned when no synchronous I/O operation is pending.
#[cfg(windows)]
const ERROR_NOT_FOUND: i32 = 1168;

/// Converts a `CancelSynchronousIo` return value into an I/O result.
#[cfg(windows)]
fn cancel_result(result: i32, error: io::Error) -> io::Result<()> {
    if result != 0 || error.raw_os_error() == Some(ERROR_NOT_FOUND) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Requests cancellation of one synchronous I/O operation on Windows.
pub(in crate::command_runner) fn cancel_synchronous_io<T>(
    handle: &JoinHandle<T>,
) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CancelSynchronousIo(thread: *mut std::ffi::c_void) -> i32;
        }

        // SAFETY: the handle belongs to this still-live helper thread. The
        // API only interrupts its current synchronous I/O call.
        let result = unsafe { CancelSynchronousIo(handle.as_raw_handle().cast()) };
        cancel_result(result, io::Error::last_os_error())
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::io;
    use std::thread::JoinHandle;

    use super::cancel_result;
    use super::cancel_synchronous_io;

    #[test]
    fn test_cancel_synchronous_io_returns_io_result() {
        let _cancel: fn(&JoinHandle<()>) -> io::Result<()> = cancel_synchronous_io;
    }

    #[test]
    fn test_cancel_result_accepts_nonzero_result() {
        let result = cancel_result(1, io::Error::from_raw_os_error(5));

        assert!(result.is_ok());
    }

    #[test]
    fn test_cancel_result_accepts_error_not_found() {
        let result = cancel_result(0, io::Error::from_raw_os_error(1168));

        assert!(result.is_ok());
    }

    #[test]
    fn test_cancel_result_preserves_other_errors() {
        let result = cancel_result(0, io::Error::from_raw_os_error(5))
            .expect_err("other cancellation errors should be preserved");

        assert_eq!(result.raw_os_error(), Some(5));
    }
}
