// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for coverage no-stdin child behavior.

use std::ffi::OsStr;

use super::coverage_support_subject;

#[test]
fn test_no_stdin_child_reports_missing_stdin_pipe() {
    let mut child = coverage_support_subject::fake_child_for(OsStr::new(
        "__qubit_command_missing_stdout__",
    ))
    .expect("synthetic no-stdin child should be created");

    assert!(child.stdin().is_none());
    assert!(child.stdout().is_none());
}
