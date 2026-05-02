/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::ffi::OsStr;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
const CSTR_EQUAL: i32 = 2;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "CompareStringOrdinal"]
    fn compare_string_ordinal(
        left: *const u16,
        left_len: i32,
        right: *const u16,
        right_len: i32,
        ignore_case: i32,
    ) -> i32;
}

/// Compares environment variable names using platform semantics.
#[cfg(not(windows))]
pub(crate) fn env_key_eq(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

/// Compares environment variable names using Windows semantics.
#[cfg(windows)]
pub(crate) fn env_key_eq(left: &OsStr, right: &OsStr) -> bool {
    let left = left.encode_wide().collect::<Vec<_>>();
    let right = right.encode_wide().collect::<Vec<_>>();
    let Ok(left_len) = i32::try_from(left.len()) else {
        return false;
    };
    let Ok(right_len) = i32::try_from(right.len()) else {
        return false;
    };
    // SAFETY: The pointers refer to the collected UTF-16 buffers and remain
    // valid for the duration of the call. The lengths are checked above.
    let comparison =
        unsafe { compare_string_ordinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    if comparison == 0 {
        log::debug!(
            "failed to compare Windows environment variable names; treating keys as distinct"
        );
    }
    comparison == CSTR_EQUAL
}
