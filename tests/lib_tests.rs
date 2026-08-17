// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for crate-level exports.

use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandError;
use qubit_command::CommandOutput;
use qubit_command::CommandRunner;
use qubit_command::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM;
use qubit_command::OutputStream;
use qubit_redact::RedactionPolicy;
use qubit_redact::Sensitivity;

#[test]
fn test_lib_exports_public_api() {
    let command = Command::new("printf").arg("hello");
    let mut builder = RedactionPolicy::default().to_builder();
    builder
        .legacy_fields()
        .raise("tenant_option", Sensitivity::Secret)
        .expect("the test policy field must be valid");
    let policy = builder
        .build()
        .expect("the diagnostic redaction policy should be valid");
    let runner = CommandRunner::new(Duration::from_secs(10))
        .diagnostic_redaction_policy(policy);
    let stream = OutputStream::Stdout;

    assert_eq!(command.program().to_string_lossy(), "printf");
    assert_eq!(runner.configured_success_exit_codes(), &[0]);
    assert_eq!(DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM, 1024 * 1024);
    assert_eq!(stream.as_str(), "stdout");
}

#[test]
fn test_lib_exports_error_and_output_types() {
    fn assert_error_type<T>()
    where
        T: std::error::Error,
    {
    }

    fn assert_output_type<T>()
    where
        T: Clone + Eq,
    {
    }

    assert_error_type::<CommandError>();
    assert_output_type::<CommandOutput>();
}
