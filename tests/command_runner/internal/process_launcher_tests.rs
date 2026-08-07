// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for process launcher behavior.

use std::time::Duration;

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

#[test]
fn test_process_launcher_maps_spawn_failure() {
    let error = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new(
            "__qubit_command_program_that_should_not_exist__",
        ))
        .expect_err("missing executable should fail to spawn");

    assert!(matches!(error, CommandError::SpawnFailed { .. }));
}
