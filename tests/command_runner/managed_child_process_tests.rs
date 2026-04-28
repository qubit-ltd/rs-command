/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for managed child process behavior.

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
fn test_managed_child_process_can_be_killed_on_timeout() {
    let error = CommandRunner::new()
        .timeout(Duration::from_millis(20))
        .run(Command::shell("sleep 1"))
        .expect_err("long-running command should time out");

    assert!(matches!(error, CommandError::TimedOut { .. }));
}
