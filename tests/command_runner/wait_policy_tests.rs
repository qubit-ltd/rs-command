// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for wait policy behavior.

#[cfg(not(windows))]
use std::time::Duration;

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
    let error = CommandRunner::new()
        .timeout(timeout)
        .run(Command::shell("sleep 1"))
        .expect_err("long-running command should time out");

    match error {
        CommandError::TimedOut {
            timeout: actual, ..
        } => assert_eq!(actual, timeout),
        other => panic!("expected timeout, got {other:?}"),
    }
}
