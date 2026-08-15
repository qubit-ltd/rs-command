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
use qubit_redact::Sensitivity;

#[test]
fn test_command_runner_default_uses_installed_global_policy() {
    let mut builder = RedactionPolicy::builder();
    builder
        .fields()
        .raise("tenant_option", Sensitivity::Secret)
        .expect("the test field should be valid");
    let policy = builder.build().expect("the test policy should build");
    RedactionPolicy::install_global(policy.clone())
        .expect("this test process installs its default only once");

    let runner = CommandRunner::new(Duration::from_secs(10));

    assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy);
}
