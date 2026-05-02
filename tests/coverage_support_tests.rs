/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage-only support hooks.

#[cfg(coverage)]
#[path = "coverage_support/failing_flush_tests.rs"]
mod failing_flush_tests;
#[cfg(coverage)]
#[path = "coverage_support/failing_reader_tests.rs"]
mod failing_reader_tests;
#[cfg(coverage)]
#[path = "coverage_support/failing_write_tests.rs"]
mod failing_write_tests;
#[cfg(coverage)]
#[path = "coverage_support/fake_child_guard_tests.rs"]
mod fake_child_guard_tests;
#[cfg(coverage)]
#[path = "coverage_support/no_stdin_child_tests.rs"]
mod no_stdin_child_tests;

#[cfg(coverage)]
#[test]
fn test_coverage_support_exposes_defensive_path_hooks() {
    let diagnostics = qubit_command::coverage_support::exercise_defensive_paths();

    assert!(!diagnostics.is_empty());
}
