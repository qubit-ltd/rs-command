/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage read-failure helper behavior.

use super::{
    FailingReader,
    OutputCaptureError,
    OutputCaptureOptions,
    read_output,
};

#[test]
fn test_failing_reader_helper_reports_read_failure() {
    let error = read_output(
        &mut FailingReader,
        OutputCaptureOptions::new(None, None, None),
    )
    .expect_err("failing reader should report read error");

    match error {
        OutputCaptureError::Read(source) => assert_eq!(source.to_string(), "read failed"),
        other => panic!("expected read error, got {other:?}"),
    }
}
