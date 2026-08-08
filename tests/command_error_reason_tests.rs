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
    assert!(open_input_debug.contains("permission denied"));
    assert!(!open_input_debug.contains(secret_input));

    let write_output = CommandErrorReason::WriteOutputFailed {
        stream: OutputStream::Stdout,
        path: secret_output.into(),
        source: std::io::Error::other("tee failed"),
    };
    let write_output_debug = format!("{write_output:?}");
    assert!(write_output_debug.contains("WriteOutputFailed"));
    assert!(write_output_debug.contains("tee failed"));
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
