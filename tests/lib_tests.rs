// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for crate-level exports.

use qubit_command::{
    Command,
    CommandError,
    CommandOutput,
    CommandRunner,
    DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM,
    OutputStream,
};
use qubit_redact::{
    RedactionPolicy,
    Sensitivity,
};

#[test]
fn test_lib_exports_public_api() {
    let command = Command::new("printf").arg("hello");
    let policy = RedactionPolicy::default()
        .to_builder()
        .raise("tenant_option", Sensitivity::Secret)
        .expect("the test policy field must be valid")
        .build()
        .expect("the diagnostic redaction policy should be valid");
    let runner = CommandRunner::new(Duration::from_secs(10)).diagnostic_redaction_policy(policy);
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
