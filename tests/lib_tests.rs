/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for crate-level exports.

use qubit_command::{
    Command,
    CommandError,
    CommandOutput,
    CommandRunner,
    OutputStream,
};

#[test]
fn test_lib_exports_public_api() {
    let command = Command::new("printf").arg("hello");
    let runner = CommandRunner::new();
    let stream = OutputStream::Stdout;

    assert_eq!(command.program().to_string_lossy(), "printf");
    assert_eq!(runner.configured_success_exit_codes(), &[0]);
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
