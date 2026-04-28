/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for output tee behavior.

use std::{
    fs,
    path::PathBuf,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use qubit_command::{
    Command,
    CommandRunner,
};

fn unique_temp_path(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "qubit-command-{name}-{}-{suffix}",
        std::process::id(),
    ))
}

#[test]
fn test_output_tee_streams_stderr_to_file() {
    let path = unique_temp_path("stderr-tee.txt");
    let output = CommandRunner::new()
        .max_stderr_bytes(5)
        .tee_stderr_to_file(path.clone())
        .run(Command::shell("rustc --version 1>&2"))
        .expect("shell command should run successfully");

    let file_bytes = fs::read(&path).expect("tee file should be readable");
    assert_eq!(output.stderr_bytes().len(), 5);
    assert!(output.stderr_truncated());
    assert!(file_bytes.starts_with(b"rustc "));

    let _ = fs::remove_file(path);
}
