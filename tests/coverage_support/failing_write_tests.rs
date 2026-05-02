/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage-only write failure support.

#[test]
fn test_failing_write_path_is_exercised() {
    let diagnostics = qubit_command::coverage_support::exercise_defensive_paths();

    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("write failed")),
    );
}
