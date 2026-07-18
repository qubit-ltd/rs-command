// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for running command behavior.

use std::time::{
    Duration,
    Instant,
};

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

#[cfg(not(windows))]
use crate::support::{
    FailingTimer,
    SwitchingTimer,
};

#[test]
fn test_running_command_completes_before_timeout() {
    let output = CommandRunner::new()
        .timeout(Duration::from_secs(5))
        .run(Command::new("rustc").arg("--version"))
        .expect("short command should finish before timeout");

    assert!(output.stdout().starts_with(b"rustc "));
}

#[cfg(not(windows))]
#[test]
fn test_running_command_reports_injected_timer_registration_failure() {
    let error = CommandRunner::new()
        .timeout(Duration::from_secs(30))
        .timer(std::sync::Arc::new(FailingTimer::new()))
        .run(Command::shell("sleep 60"))
        .expect_err("timer registration failure should stop the command");

    assert!(matches!(error, CommandError::TimeFailed { .. }));
}

#[cfg(not(windows))]
#[test]
fn test_running_command_reports_clock_domain_change_after_exit() {
    let error = CommandRunner::new()
        .without_timeout()
        .timer(std::sync::Arc::new(SwitchingTimer::after_observations(2)))
        .run(Command::shell("printf done"))
        .expect_err("changing timer domains should reject elapsed time");

    assert!(matches!(error, CommandError::TimeFailed { .. }));
}

#[cfg(not(windows))]
#[test]
fn test_running_command_rejects_inconsistent_timer_before_waiting() {
    let started = Instant::now();
    let error = CommandRunner::new()
        .without_timeout()
        .timer(std::sync::Arc::new(SwitchingTimer::new()))
        .run(Command::shell("sleep 1").stdin_bytes(b"ignored".to_vec()))
        .expect_err("inconsistent timer should be rejected during startup");

    assert!(matches!(error, CommandError::TimeFailed { .. }));
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[cfg(not(windows))]
#[test]
fn test_running_command_cleans_up_inherited_output_after_time_failure() {
    let error = CommandRunner::new()
        .timeout(Duration::from_secs(30))
        .timer(std::sync::Arc::new(SwitchingTimer::after_observations(2)))
        .run(Command::shell("printf done; sleep 1 &"))
        .expect_err("changing timer domains should stop output collection");

    assert!(matches!(error, CommandError::TimeFailed { .. }));
}
