// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for wait policy behavior.

#[cfg(not(windows))]
use std::{
    thread,
    time::Duration,
};

#[cfg(not(windows))]
use qubit_clock::{
    ManualMonotonicClock,
    MonotonicClock,
};
#[cfg(not(windows))]
use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

#[cfg(not(windows))]
#[test]
fn test_wait_policy_enforces_configured_timeout() {
    let timeout = Duration::from_millis(20);
    let error = CommandRunner::new(timeout)
        .run(Command::shell("sleep 1"))
        .expect_err("long-running command should time out");

    match error {
        CommandError::TimedOut {
            timeout: actual, ..
        } => assert_eq!(actual, timeout),
        other => panic!("expected timeout, got {other:?}"),
    }
}

#[cfg(not(windows))]
#[test]
fn test_wait_policy_starts_timeout_polling_with_short_interval() {
    let clock = ManualMonotonicClock::new_shared();
    let runner =
        CommandRunner::new(Duration::from_secs(30)).timer(clock.new_timer());
    let worker = thread::spawn(move || runner.run(Command::shell("sleep 60")));

    assert!(clock.wait_for_waiters(1, Duration::from_secs(2)));
    let deadline = clock
        .next_deadline()
        .expect("timeout polling should register a deadline");
    assert_eq!(
        Duration::from_millis(1),
        deadline
            .duration_since(clock.now())
            .expect("deadline should use the same clock domain"),
    );

    clock
        .advance(Duration::from_secs(30))
        .expect("manual clock should advance to the timeout");
    let error = worker
        .join()
        .expect("runner thread should not panic")
        .expect_err("long-running command should time out");
    assert!(matches!(error, CommandError::TimedOut { .. }));
}
