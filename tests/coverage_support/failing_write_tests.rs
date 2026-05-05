/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage write-failure helper behavior.

use std::io;

use super::{
    FailingWrite,
    OutputCaptureError,
    capture_with_writer,
    read_output,
};

#[test]
fn test_failing_write_helper_reports_write_failure() {
    let error = read_output(
        &mut io::Cursor::new(b"write".to_vec()),
        capture_with_writer(Box::new(FailingWrite), "stdout.txt"),
    )
    .expect_err("failing write should be reported");

    match error {
        OutputCaptureError::Write { path, source } => {
            assert_eq!(path.display().to_string(), "stdout.txt");
            assert_eq!(source.to_string(), "write failed");
        }
        other => panic!("expected tee write error, got {other:?}"),
    }
}
