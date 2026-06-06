// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for coverage synthetic children.

use std::ffi::OsStr;

use super::{
    OutputStream,
    coverage_support_subject,
};

#[test]
fn test_synthetic_children_match_only_magic_program_names() {
    assert!(
        coverage_support_subject::fake_child_for(OsStr::new(
            "__qubit_command_missing_stdout__"
        ))
        .is_some()
    );
    assert!(
        coverage_support_subject::fake_child_for(OsStr::new("rustc")).is_none()
    );
}

#[test]
fn test_synthetic_children_force_collect_output_for_magic_commands() {
    assert_eq!(
        coverage_support_subject::forced_collect_output_error(
            "[\"__qubit_command_collect_output_error__\"]",
        ),
        Some(OutputStream::Stdout),
    );
    assert_eq!(
        coverage_support_subject::forced_collect_output_error("[\"rustc\"]"),
        None,
    );
}
