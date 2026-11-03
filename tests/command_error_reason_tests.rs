// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandErrorReason`](qubit_command::CommandErrorReason).

use qubit_command::Command;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunner;
use qubit_command::OutputStream;

#[test]
fn test_command_error_reason_retains_unexpected_exit_details() {
    let error = CommandRunner::without_timeout()
        .run(Command::shell("exit 7"))
        .expect_err("non-zero exit should produce a reason");
    assert!(matches!(
        error.reason(),
        CommandErrorReason::UnexpectedExit {
            exit_code: Some(7),
            expected,
        } if expected == &[0]
    ));
}

#[test]
fn test_command_error_reason_debug_redacts_paths() {
    let secret_input = "/secret/input/credentials.txt";
    let secret_output = "/secret/output/diagnostic.log";
    let source = std::io::Error::other("permission denied");

    let open_input = CommandErrorReason::OpenInputFailed {
        path: secret_input.into(),
        source,
    };
    let open_input_debug = format!("{open_input:?}");
    assert!(open_input_debug.contains("OpenInputFailed"));
    assert!(!open_input_debug.contains("permission denied"));
    assert!(open_input_debug.contains("kind=Other"));
    assert!(open_input_debug.contains("raw_os_error=None"));
    assert!(!open_input_debug.contains(secret_input));

    let write_output = CommandErrorReason::WriteOutputFailed {
        stream: OutputStream::Stdout,
        path: secret_output.into(),
        source: std::io::Error::other("tee failed"),
    };
    let write_output_debug = format!("{write_output:?}");
    assert!(write_output_debug.contains("WriteOutputFailed"));
    assert!(!write_output_debug.contains("tee failed"));
    assert!(write_output_debug.contains("kind=Other"));
    assert!(!write_output_debug.contains(secret_output));

    let conflict = CommandErrorReason::InputOutputConflict {
        input_path: secret_input.into(),
        output_stream: OutputStream::Stdout,
        output_path: secret_output.into(),
    };
    let conflict_debug = format!("{conflict:?}");
    assert!(conflict_debug.contains("InputOutputConflict"));
    assert!(!conflict_debug.contains(secret_input));
    assert!(!conflict_debug.contains(secret_output));
}

#[test]
fn test_command_error_reason_debug_preserves_raw_os_error_metadata() {
    let reason = CommandErrorReason::SpawnFailed {
        source: std::io::Error::from_raw_os_error(13),
    };
    let debug = format!("{reason:?}");
    assert!(debug.contains("kind="));
    assert!(debug.contains("raw_os_error=Some(13)"));
}

#[test]
fn test_command_error_reason_debug_does_not_format_custom_source() {
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

    let reason = CommandErrorReason::SpawnFailed {
        source: std::io::Error::other(PanicSource),
    };
    let debug = format!("{reason:?}");
    assert!(debug.contains("SpawnFailed"));
    assert!(debug.contains("kind=Other"));
}

#[test]
fn test_every_io_source_reason_uses_a_controlled_summary() {
    let secret = "source-secret-/private/token";
    let source = || std::io::Error::other(secret);
    let reasons = [
        CommandErrorReason::SpawnFailed { source: source() },
        CommandErrorReason::WaitFailed { source: source() },
        CommandErrorReason::ReadOutputFailed {
            stream: OutputStream::Stdout,
            source: source(),
        },
        CommandErrorReason::ReadOutputFailed {
            stream: OutputStream::Stderr,
            source: source(),
        },
        CommandErrorReason::OpenInputFailed {
            path: "input".into(),
            source: source(),
        },
        CommandErrorReason::OpenOutputFailed {
            stream: OutputStream::Stdout,
            path: "stdout".into(),
            source: source(),
        },
        CommandErrorReason::OpenOutputFailed {
            stream: OutputStream::Stderr,
            path: "stderr".into(),
            source: source(),
        },
        CommandErrorReason::InspectIoFileFailed {
            path: "file".into(),
            source: source(),
        },
        CommandErrorReason::StartInputThreadFailed { source: source() },
        CommandErrorReason::StartOutputThreadFailed {
            stream: OutputStream::Stdout,
            source: source(),
        },
        CommandErrorReason::StartOutputThreadFailed {
            stream: OutputStream::Stderr,
            source: source(),
        },
        CommandErrorReason::WriteInputFailed { source: source() },
        CommandErrorReason::WriteOutputFailed {
            stream: OutputStream::Stdout,
            path: "stdout".into(),
            source: source(),
        },
        CommandErrorReason::WriteOutputFailed {
            stream: OutputStream::Stderr,
            path: "stderr".into(),
            source: source(),
        },
    ];

    for reason in reasons {
        let debug = format!("{reason:?}");
        assert!(!debug.contains(secret), "{debug}");
        assert!(debug.contains("kind=Other"), "{debug}");
    }
}

#[test]
fn test_time_error_reason_debug_omits_source_details() {
    let reason = CommandErrorReason::TimeFailed {
        source: qubit_clock::TimeError::InstantOverflow,
    };
    let debug = format!("{reason:?}");
    assert!(debug.contains("TimeFailed"));
    assert!(debug.contains("<time error details omitted>"));
    assert!(!debug.contains("InstantOverflow"));
}
