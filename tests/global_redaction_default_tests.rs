// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests that command-runner defaults snapshot the application policy.

use std::sync::Mutex;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandRunner;
use qubit_redact::RedactionPolicy;
use qubit_redact::Redactor;
use qubit_redact::Sensitivity;

static APPLICATION_DEFAULT_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_command_runner_default_uses_installed_global_policy() {
    let _guard = APPLICATION_DEFAULT_LOCK
        .lock()
        .expect("test lock should not be poisoned");
    let policy = RedactionPolicy::builder()
        .fields(|fields| {
            fields.raise("tenant_option", Sensitivity::Secret);
        })
        .expect("the test field should be valid")
        .build()
        .expect("the test policy should build");
    let previous = Redactor::replace_application_default(Redactor::new(policy.clone()));

    let runner = CommandRunner::new(Duration::from_secs(10));

    assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy);
    let _ = Redactor::replace_application_default(previous);
}

/// Command diagnostics keep argv, environment, and removed variables in one
/// batch, so late fields do not receive a fresh output allowance.
#[test]
fn test_command_debug_shares_one_redaction_budget() {
    let _guard = APPLICATION_DEFAULT_LOCK
        .lock()
        .expect("test lock should not be poisoned");
    let policy = RedactionPolicy::builder()
        .limits(|limits| {
            limits.max_output_bytes(16);
        })
        .expect("limits should be valid")
        .build()
        .expect("policy should build");
    let previous = Redactor::replace_application_default(Redactor::new(policy));

    let debug = format!(
        "{:?}",
        Command::new("program-name-that-exhausts-budget")
            .env("LATE_ENV", "late-value")
            .env_remove("LATE_UNSET"),
    );
    let _ = Redactor::replace_application_default(previous);

    assert!(!debug.contains("LATE_ENV"));
    assert!(!debug.contains("LATE_UNSET"));
}
