/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage flush-failure helper behavior.

use std::io;

use super::{
    FailingFlush,
    OutputCaptureError,
    capture_with_writer,
    read_output,
};

#[test]
fn test_failing_flush_helper_reports_flush_failure() {
    let error = read_output(
        &mut io::Cursor::new(b"flush".to_vec()),
        capture_with_writer(Box::new(FailingFlush), "stderr.txt"),
    )
    .expect_err("failing flush should be reported");

    match error {
        OutputCaptureError::Write { path, source } => {
            assert_eq!(path.display().to_string(), "stderr.txt");
            assert_eq!(source.to_string(), "flush failed");
        }
        other => panic!("expected flush write error, got {other:?}"),
    }
}
