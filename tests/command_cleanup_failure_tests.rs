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
        !debug.contains("tee failed"),
        "debug output should omit arbitrary source text"
    );
    assert!(debug.contains("kind=Other"));
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
        !debug.contains("cancel failed"),
        "debug output should omit arbitrary source text"
    );
    assert!(debug.contains("kind=Other"));
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
        !debug.contains("cancel failed"),
        "debug output should omit arbitrary source text"
    );
    assert!(debug.contains("kind=Other"));
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
        !debug.contains("cancel failed"),
        "debug output should omit arbitrary source text"
    );
    assert!(debug.contains("kind=Other"));
}

#[test]
fn test_time_cleanup_debug_omits_source_details() {
    let failure = CommandCleanupFailure::Time {
        source: qubit_clock::TimeError::InstantOverflow,
    };
    let debug = format!("{failure:?}");
    assert!(debug.contains("Time"));
    assert!(debug.contains("<time error details omitted>"));
}

#[test]
fn test_cleanup_failure_debug_does_not_format_custom_source() {
    struct PanicSource;
    impl std::fmt::Display for PanicSource {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            panic!("source Display must not run")
        }
    }
    impl std::fmt::Debug for PanicSource {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            panic!("source Debug must not run")
        }
    }
    impl std::error::Error for PanicSource {}

    let failure = CommandCleanupFailure::StderrCancellation {
        source: io::Error::other(PanicSource),
    };
    let debug = format!("{failure:?}");
    assert!(debug.contains("StderrCancellation"));
    assert!(debug.contains("kind=Other"));
}

#[test]
fn test_cleanup_failure_preserves_raw_os_error() {
    let failure = CommandCleanupFailure::Wait {
        source: io::Error::from_raw_os_error(13),
    };
    assert!(matches!(failure, CommandCleanupFailure::Wait { source } if source.raw_os_error() == Some(13)));
}

#[test]
fn test_every_cleanup_io_source_uses_a_controlled_summary() {
    let secret = "source-secret-/private/token";
    let source = || io::Error::other(secret);
    let failures = [
        CommandCleanupFailure::Wait { source: source() },
        CommandCleanupFailure::ProcessTreeTermination { source: source() },
        CommandCleanupFailure::ChildTermination { source: source() },
        CommandCleanupFailure::Stdin { source: source() },
        CommandCleanupFailure::StdinCancellation { source: source() },
        CommandCleanupFailure::StdoutCancellation { source: source() },
        CommandCleanupFailure::StderrCancellation { source: source() },
        CommandCleanupFailure::StdoutRead { source: source() },
        CommandCleanupFailure::StdoutWrite {
            path: "stdout".into(),
            source: source(),
        },
        CommandCleanupFailure::StderrRead { source: source() },
        CommandCleanupFailure::StderrWrite {
            path: "stderr".into(),
            source: source(),
        },
    ];

    for failure in failures {
        let debug = format!("{failure:?}");
        assert!(!debug.contains(secret), "{debug}");
        assert!(debug.contains("kind=Other"), "{debug}");
    }
}
