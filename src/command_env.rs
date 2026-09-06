// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
const CSTR_EQUAL: i32 = 2;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    /// Compares two explicit-length UTF-16 strings using Windows ordinal rules.
    ///
    /// The integer lengths describe the buffers addressed by `left` and
    /// `right`; a nonzero `ignore_case` requests case-insensitive comparison.
    ///
    /// # Parameters
    ///
    /// * `left` - Pointer to the first UTF-16 buffer.
    /// * `left_len` - Number of UTF-16 code units in `left`.
    /// * `right` - Pointer to the second UTF-16 buffer.
    /// * `right_len` - Number of UTF-16 code units in `right`.
    /// * `ignore_case` - Nonzero to compare without case sensitivity.
    ///
    /// # Returns
    ///
    /// Windows ordinal comparison result, or zero when the API reports an
    /// error.
    ///
    /// # Safety
    ///
    /// Both pointers must remain valid for their declared lengths throughout
    /// the call.
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
///
/// # Parameters
///
/// * `left` - First environment variable name.
/// * `right` - Second environment variable name.
///
/// # Returns
///
/// `true` when the names are byte-for-byte equal.
#[cfg(not(windows))]
#[must_use]
#[inline(always)]
pub(crate) fn env_key_eq(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

/// Compares environment variable names using Windows semantics.
///
/// # Parameters
///
/// * `left` - First environment variable name.
/// * `right` - Second environment variable name.
///
/// # Returns
///
/// `true` when Windows ordinal case-insensitive comparison reports equality;
/// `false` when either length exceeds the platform API or comparison fails.
#[cfg(windows)]
#[must_use]
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
    let comparison = unsafe { compare_string_ordinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    if comparison == 0 {
        log::debug!("failed to compare Windows environment variable names; treating keys as distinct");
    }
    comparison == CSTR_EQUAL
}
