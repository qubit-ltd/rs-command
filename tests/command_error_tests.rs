// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandError`](qubit_command::CommandError).

use std::error::Error;

use qubit_command::Command;
use qubit_command::CommandCancellation;
use qubit_command::CommandErrorKind;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;

mod support;
use support::output_command;

#[test]
fn test_command_error_kind_and_reason_are_stable() {
    let spawn = CommandRunner::without_timeout()
        .run(Command::new("__qubit_command_missing_executable__"))
        .expect_err("missing executable should fail");
    assert_eq!(spawn.kind(), CommandErrorKind::SpawnFailed);
    assert!(matches!(spawn.reason(), CommandErrorReason::SpawnFailed { .. }));
    assert!(spawn.source().is_some());
    assert!(spawn.cleanup_failures().is_empty());

    let unexpected = CommandRunner::without_timeout()
        .run(output_command(b"output", b"", 9))
        .expect_err("non-zero exit should fail");
    assert_eq!(unexpected.kind(), CommandErrorKind::UnexpectedExit);
    assert!(unexpected.is_unexpected_exit());
    assert_eq!(unexpected.exit_code(), Some(9));
    assert_eq!(unexpected.output().expect("output").stdout(), b"output");
    assert!(matches!(
        unexpected.reason(),
        CommandErrorReason::UnexpectedExit { exit_code: Some(9), .. }
    ));

    let truncated = CommandRunner::without_timeout()
        .max_stdout_bytes(3)
        .run(output_command(b"output", b"", 0))
        .expect_err("truncated output should fail");
    assert_eq!(truncated.kind(), CommandErrorKind::OutputTruncated);
    assert!(!truncated.is_unexpected_exit());
    assert_eq!(truncated.output().expect("output").stdout(), b"out");
    assert_eq!(truncated.into_output().expect("output").stdout(), b"out");
}

#[test]
fn test_command_error_cancelled_before_start_has_no_output() {
    let cancellation = CommandCancellation::new();
    cancellation.cancel();
    let error = CommandRunner::without_timeout()
        .run_with(
            Command::new("__qubit_command_must_not_start__"),
            CommandRunOptions::new().cancellation(cancellation),
        )
        .expect_err("pre-cancelled command should fail");
    assert_eq!(error.kind(), CommandErrorKind::CancelledBeforeStart);
    assert!(matches!(error.reason(), CommandErrorReason::CancelledBeforeStart));
    assert!(error.output().is_none());
}

#[test]
fn test_command_error_debug_redacts_output_and_paths() {
    let error = CommandRunner::without_timeout()
        .run(output_command(b"stdout-secret", b"stderr-secret", 7))
        .expect_err("command should fail");
    let debug = format!("{error:?}");
    let stdout_debug = format!("{:?}", b"stdout-secret".to_vec());
    let stderr_debug = format!("{:?}", b"stderr-secret".to_vec());
    assert!(!debug.contains("stdout-secret"));
    assert!(!debug.contains("stderr-secret"));
    assert!(!debug.contains(&stdout_debug));
    assert!(!debug.contains(&stderr_debug));
}
