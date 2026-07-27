// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::time::Duration;

/// Timeout polling delays from the first check through the steady-state cap.
const TIMEOUT_POLL_INTERVALS: [Duration; 5] = [
    Duration::from_millis(1),
    Duration::from_millis(2),
    Duration::from_millis(4),
    Duration::from_millis(8),
    Duration::from_millis(10),
];

/// Index of the steady-state timeout polling delay.
const LAST_TIMEOUT_POLL_INTERVAL_INDEX: usize =
    TIMEOUT_POLL_INTERVALS.len() - 1;

/// Calculates how long to sleep before polling the child again.
///
/// # Parameters
///
/// * `timeout` - Total timeout.
/// * `elapsed` - Elapsed command time.
/// * `poll_count` - Number of unsuccessful timeout polls already completed.
///
/// # Returns
///
/// The lesser of the adaptive polling interval and the remaining timeout.
#[must_use]
#[inline]
pub(in crate::command_runner) fn next_sleep(
    timeout: Duration,
    elapsed: Duration,
    poll_count: usize,
) -> Duration {
    let interval = TIMEOUT_POLL_INTERVALS
        [poll_count.min(LAST_TIMEOUT_POLL_INTERVAL_INDEX)];
    timeout.saturating_sub(elapsed).min(interval)
}
