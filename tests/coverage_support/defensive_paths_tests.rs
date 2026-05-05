/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage defensive-path helpers.

use std::{
    io,
    time::Duration,
};

use super::{
    CommandError,
    OutputStream,
    kill_failed,
    output_pipe_error,
    spawn_failed,
    wait_failed,
};

#[test]
fn test_defensive_paths_return_diagnostics_for_expected_failures() {
    assert!(matches!(
        spawn_failed("spawn", io::Error::other("spawn failed")),
        CommandError::SpawnFailed { .. },
    ));
    assert!(matches!(
        wait_failed("wait", io::Error::other("wait failed")),
        CommandError::WaitFailed { .. },
    ));
    assert!(matches!(
        kill_failed(
            "kill".to_owned(),
            Duration::from_millis(1),
            io::Error::other("kill failed"),
        ),
        CommandError::KillFailed { .. },
    ));
    assert!(matches!(
        output_pipe_error("pipe", OutputStream::Stdout),
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stdout,
            ..
        },
    ));
}
