// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::time::Duration;

/// Polling interval used while waiting for a child process with timeout.
pub(crate) const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Calculates how long to sleep before polling the child again.
///
/// # Parameters
///
/// * `timeout` - Total timeout.
/// * `elapsed` - Elapsed command time.
///
/// # Returns
///
/// The lesser of the polling interval and the remaining timeout.
#[must_use]
#[inline]
pub(crate) fn next_sleep(timeout: Duration, elapsed: Duration) -> Duration {
    timeout.saturating_sub(elapsed).min(WAIT_POLL_INTERVAL)
}
