// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for per-run [`CommandRunOptions`](qubit_command::CommandRunOptions).

use std::path::Path;

use qubit_command::CommandCancellation;
use qubit_command::CommandRunOptions;

#[test]
fn test_command_run_options_store_cancellation_and_tee_paths() {
    let cancellation = CommandCancellation::new();
    let options = CommandRunOptions::new()
        .cancellation(cancellation.clone())
        .tee_stdout_to_file("stdout.log")
        .tee_stderr_to_file("stderr.log");

    assert!(
        options
            .configured_cancellation()
            .is_some_and(|value| !value.is_cancelled())
    );
    cancellation.cancel();
    assert!(
        options
            .configured_cancellation()
            .is_some_and(CommandCancellation::is_cancelled)
    );
    assert_eq!(
        options.configured_stdout_file(),
        Some(Path::new("stdout.log"))
    );
    assert_eq!(
        options.configured_stderr_file(),
        Some(Path::new("stderr.log"))
    );
}

#[test]
fn test_command_run_options_clone_copies_tee_paths() {
    let options = CommandRunOptions::new()
        .tee_stdout_to_file("stdout.log")
        .tee_stderr_to_file("stderr.log");
    let clone = options.clone();

    assert_eq!(
        clone.configured_stdout_file(),
        options.configured_stdout_file()
    );
    assert_eq!(
        clone.configured_stderr_file(),
        options.configured_stderr_file()
    );
}
