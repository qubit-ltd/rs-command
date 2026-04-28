/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for coverage-only flush failure support.

#[test]
fn test_failing_flush_path_is_exercised() {
    let diagnostics = qubit_command::coverage_support::exercise_defensive_paths();

    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("flush failed")),
    );
}
