/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::time::Duration;

/// Polling interval used while waiting for a child process with timeout.
pub(crate) const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Calculates how long to sleep before polling the child again.
pub(crate) fn next_sleep(timeout: Option<Duration>, elapsed: Duration) -> Duration {
    if let Some(timeout) = timeout
        && let Some(remaining) = timeout.checked_sub(elapsed)
    {
        return remaining.min(WAIT_POLL_INTERVAL);
    }
    WAIT_POLL_INTERVAL
}
