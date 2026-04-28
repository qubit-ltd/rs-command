/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for coverage-only read failure support.

#[test]
fn test_failing_reader_path_is_exercised() {
    let diagnostics = qubit_command::coverage_support::exercise_defensive_paths();

    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("read failed")),
    );
}
