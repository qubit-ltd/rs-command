// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests that command-runner defaults snapshot the application policy.

use std::time::Duration;

use qubit_command::CommandRunner;
use qubit_redact::RedactionPolicy;
use qubit_redact::Redactor;
use qubit_redact::Sensitivity;

#[test]
fn test_command_runner_default_uses_installed_global_policy() {
    let policy = RedactionPolicy::builder()
        .fields(|fields| {
            fields.raise("tenant_option", Sensitivity::Secret);
        })
        .expect("the test field should be valid")
        .build()
        .expect("the test policy should build");
    let previous =
        Redactor::replace_application_default(Redactor::new(policy.clone()));

    let runner = CommandRunner::new(Duration::from_secs(10));

    assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy);
    let _ = Redactor::replace_application_default(previous);
}
