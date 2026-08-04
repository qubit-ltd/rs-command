// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Tests that command-runner defaults snapshot the application policy.

use qubit_command::CommandRunner;
use qubit_redact::{
    RedactionPolicy,
    Sensitivity,
};

#[test]
fn test_command_runner_default_uses_installed_global_policy() {
    let policy = RedactionPolicy::builder()
        .raise("tenant_option", Sensitivity::Secret)
        .expect("the test field should be valid")
        .build()
        .expect("the test policy should build");
    RedactionPolicy::install_global(policy.clone())
        .expect("this test process installs its default only once");

    let runner = CommandRunner::default();

    assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy);
}
