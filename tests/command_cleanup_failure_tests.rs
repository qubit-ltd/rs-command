// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandCleanupFailure`](qubit_command::CommandCleanupFailure).

use std::fmt::Debug;
use std::io;

use qubit_command::CommandCleanupFailure;

#[test]
fn test_command_cleanup_failure_is_debuggable() {
    fn assert_debug<T: Debug>() {}

    assert_debug::<CommandCleanupFailure>();
}

#[test]
fn test_command_cleanup_failure_debug_redacts_paths() {
    let secret_path = "/secret/output/diagnostic.log";
    let failure = CommandCleanupFailure::StdoutWrite {
        path: secret_path.into(),
        source: io::Error::other("tee failed"),
    };

    let debug = format!("{failure:?}");
    assert!(
        debug.contains("StdoutWrite"),
        "debug output should identify the failure variant"
    );
    assert!(
        debug.contains("tee failed"),
        "debug output should preserve the source error"
    );
    assert!(
        !debug.contains(secret_path),
        "debug output should redact the configured path"
    );
}

#[test]
fn test_stdin_cancellation_cleanup_failure_debug_includes_variant_and_error() {
    let failure = CommandCleanupFailure::StdinCancellation {
        source: io::Error::other("cancel failed"),
    };

    let debug = format!("{failure:?}");
    assert!(
        debug.contains("StdinCancellation"),
        "debug output should identify the failure variant"
    );
    assert!(
        debug.contains("cancel failed"),
        "debug output should preserve the source error"
    );
}

#[test]
fn test_stdout_cancellation_cleanup_failure_debug_includes_variant_and_error() {
    let failure = CommandCleanupFailure::StdoutCancellation {
        source: io::Error::other("cancel failed"),
    };

    let debug = format!("{failure:?}");
    assert!(
        debug.contains("StdoutCancellation"),
        "debug output should identify the failure variant"
    );
    assert!(
        debug.contains("cancel failed"),
        "debug output should preserve the source error"
    );
}

#[test]
fn test_stderr_cancellation_cleanup_failure_debug_includes_variant_and_error() {
    let failure = CommandCleanupFailure::StderrCancellation {
        source: io::Error::other("cancel failed"),
    };

    let debug = format!("{failure:?}");
    assert!(
        debug.contains("StderrCancellation"),
        "debug output should identify the failure variant"
    );
    assert!(
        debug.contains("cancel failed"),
        "debug output should preserve the source error"
    );
}
