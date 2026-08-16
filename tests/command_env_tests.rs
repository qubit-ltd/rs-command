// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for command environment behavior.

use std::ffi::OsStr;

use qubit_command::Command;
use qubit_redact::InputOutputLimit;
use qubit_redact::RedactionCompletion;
use qubit_redact::RedactionPolicy;
use qubit_redact::Redactor;
use qubit_redact::formats::argv::ArgvItem;

#[test]
fn test_command_env_readding_removed_key_clears_removal() {
    let command = Command::new("env")
        .env_remove("QUBIT_COMMAND_ENV_TEST")
        .env("QUBIT_COMMAND_ENV_TEST", "restored");

    assert!(command.removed_environment().is_empty());
    assert_eq!(command.environment().len(), 1);
    assert_eq!(
        command.environment()[0].0.to_string_lossy(),
        "QUBIT_COMMAND_ENV_TEST",
    );
    assert_eq!(command.environment()[0].1.to_string_lossy(), "restored");
}

/// Verifies incomplete unset redaction is mapped from completion state.
#[test]
fn test_command_debug_maps_incomplete_unset_redaction_to_marker() {
    let limit = InputOutputLimit::builder()
        .max_input_bytes(512)
        .max_output_bytes(InputOutputLimit::MIN_OUTPUT_BYTES + 7)
        .build()
        .expect("the test diagnostic limit should be valid");
    let mut builder = RedactionPolicy::builder();
    builder.limits().diagnostic_event(limit);
    let policy = builder.build().expect("the test policy should build");
    let oversized_program = "P".repeat(513);
    let oversized_removed = "R".repeat(512);
    let oversized_value = "V".repeat(128);

    let redactor = Redactor::new(policy.clone());
    let mut session = redactor.session();
    let _ = session
        .argv()
        .redact_items([ArgvItem::plain(OsStr::new("x"))]);
    let _ = session.env().redact_os_pairs(std::iter::empty());
    let truncated = session
        .argv()
        .redact_items([ArgvItem::plain(OsStr::new(&oversized_removed))]);
    assert!(matches!(
        truncated.completion(),
        RedactionCompletion::Truncated
    ));

    let redactor = Redactor::new(policy.clone());
    let mut session = redactor.session();
    let _ = session
        .argv()
        .redact_items([ArgvItem::plain(OsStr::new("x"))]);
    let _ = session
        .env()
        .redact_os_pairs([(OsStr::new("A"), OsStr::new(&oversized_value))]);
    let exhausted = session
        .argv()
        .redact_items([ArgvItem::plain(OsStr::new("REMOVED"))]);
    assert!(matches!(
        exhausted.completion(),
        RedactionCompletion::Exhausted
    ));

    RedactionPolicy::install_global(policy)
        .expect("this test process installs its default only once");
    let truncated_debug =
        format!("{:?}", Command::new("x").env_remove(&oversized_removed));
    let argv_truncated_debug = format!(
        "{:?}",
        Command::new(&oversized_program)
            .env("MODE", "debug")
            .env_remove("OLD_MODE"),
    );
    let exhausted_debug = format!(
        "{:?}",
        Command::new("x")
            .env("A", &oversized_value)
            .env_remove("REMOVED"),
    );

    assert!(truncated_debug.contains("unset: <truncated>"));
    assert!(!truncated_debug.contains(r#"unset: ["<truncated>"]"#));
    assert!(exhausted_debug.contains("unset: <truncated>"));
    assert!(argv_truncated_debug.contains("argv: <truncated>"));
    assert!(!argv_truncated_debug.contains(r#"argv: ["<truncated>"]"#));
    assert!(argv_truncated_debug.contains("env: <truncated>"));
    assert!(argv_truncated_debug.contains("unset: <truncated>"));
}
