// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::time::Duration;

/// Adaptive polling delays from the first check through the steady-state cap.
const ADAPTIVE_POLL_INTERVALS: [Duration; 5] = [
    Duration::from_millis(1),
    Duration::from_millis(2),
    Duration::from_millis(4),
    Duration::from_millis(8),
    Duration::from_millis(10),
];

/// Index of the steady-state polling delay.
const LAST_ADAPTIVE_POLL_INTERVAL_INDEX: usize = ADAPTIVE_POLL_INTERVALS.len() - 1;

/// Returns the adaptive delay for the given number of unsuccessful polls.
///
/// # Parameters
///
/// * `poll_count` - Number of unsuccessful polls already completed.
///
/// # Returns
///
/// The polling delay, capped at 10 milliseconds.
#[must_use]
#[inline]
pub(in crate::command_runner) fn next_poll_interval(poll_count: usize) -> Duration {
    ADAPTIVE_POLL_INTERVALS[poll_count.min(LAST_ADAPTIVE_POLL_INTERVAL_INDEX)]
}

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
pub(in crate::command_runner) fn next_sleep(timeout: Duration, elapsed: Duration, poll_count: usize) -> Duration {
    timeout.saturating_sub(elapsed).min(next_poll_interval(poll_count))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::next_poll_interval;

    #[test]
    fn test_next_poll_interval_grows_to_ten_milliseconds_and_stays_capped() {
        assert_eq!(next_poll_interval(0), Duration::from_millis(1));
        assert_eq!(next_poll_interval(1), Duration::from_millis(2));
        assert_eq!(next_poll_interval(2), Duration::from_millis(4));
        assert_eq!(next_poll_interval(3), Duration::from_millis(8));
        assert_eq!(next_poll_interval(4), Duration::from_millis(10));
        assert_eq!(next_poll_interval(5), Duration::from_millis(10));
    }
}
