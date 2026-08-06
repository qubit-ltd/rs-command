// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.
// =============================================================================
//! Platform support for interrupting synchronous helper-thread I/O.

use std::thread::JoinHandle;

/// Requests cancellation of one synchronous I/O operation on Windows.
pub(in crate::command_runner) fn cancel_synchronous_io<T>(
    handle: &JoinHandle<T>,
) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CancelSynchronousIo(thread: *mut std::ffi::c_void) -> i32;
        }

        // SAFETY: the handle belongs to this still-live helper thread. The
        // API only interrupts its current synchronous I/O call.
        unsafe {
            let _ = CancelSynchronousIo(handle.as_raw_handle().cast());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
    }
}
