/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for output capture error mapping.

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
    OutputStream,
};

#[test]
fn test_output_capture_error_reports_unopenable_stdout_file() {
    let missing_directory = std::env::temp_dir().join("qubit-command-missing-output-directory");
    let path = missing_directory.join("stdout.txt");
    let error = CommandRunner::new()
        .tee_stdout_to_file(path)
        .run(Command::new("rustc").arg("--version"))
        .expect_err("missing output directory should be reported");

    assert!(matches!(
        error,
        CommandError::OpenOutputFailed {
            stream: OutputStream::Stdout,
            ..
        },
    ));
}
