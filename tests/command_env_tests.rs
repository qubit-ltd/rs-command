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

/// Verifies finite adapter results are staged and published on finish.
#[test]
fn test_command_debug_stages_named_adapter_results() {
    let redactor = Redactor::new(RedactionPolicy::default());
    let mut batch = redactor.batch();
    let argv = batch.redact_argv([ArgvItem::plain(OsStr::new("x"))]);
    let env = batch.redact_env_pairs(std::iter::empty());
    let unset = batch.redact_argv([ArgvItem::plain(OsStr::new("REMOVED"))]);
    let output = batch.finish();
    assert!(output.resolve(argv).is_ok());
    assert!(output.resolve(env).is_ok());
    assert!(output.resolve(unset).is_ok());
}
