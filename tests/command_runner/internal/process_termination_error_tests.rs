// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for process-termination error mapping.

#[cfg(not(windows))]
use std::{
    thread,
    time::Duration,
};

#[cfg(not(windows))]
use qubit_command::{
    Command,
    CommandCancellation,
    CommandError,
    CommandRunOptions,
    CommandRunner,
};

#[cfg(not(windows))]
#[test]
fn test_process_termination_maps_timeout_after_kill() {
    let error = CommandRunner::new(Duration::from_millis(20))
        .run(Command::shell("sleep 1"))
        .expect_err("terminated command should report its timeout");

    assert!(matches!(error, CommandError::TimedOut { .. }));
}

#[cfg(not(windows))]
#[test]
fn test_process_termination_maps_cancellation_after_kill() {
    let cancellation = CommandCancellation::new();
    let cancellation_request = cancellation.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        cancellation_request.cancel();
    });

    let error = CommandRunner::without_timeout()
        .run_with(
            Command::shell("sleep 1"),
            CommandRunOptions::new().cancellation(cancellation),
        )
        .expect_err("terminated command should report cancellation");
    canceller
        .join()
        .expect("cancellation request thread should finish");

    assert!(matches!(error, CommandError::Cancelled { .. }));
}
