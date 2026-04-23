/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for [`OutputStream`](qubit_command::OutputStream).

use qubit_command::OutputStream;

#[test]
fn test_output_stream_formats_name() {
    assert_eq!(OutputStream::Stdout.as_str(), "stdout");
    assert_eq!(OutputStream::Stderr.as_str(), "stderr");
    assert_eq!(OutputStream::Stdout.to_string(), "stdout");
    assert_eq!(OutputStream::Stderr.to_string(), "stderr");
}
