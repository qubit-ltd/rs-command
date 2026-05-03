/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for output capture options.

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
fn test_output_capture_options_keep_full_tee_with_limited_memory() {
    let path = unique_temp_path("stdout-capture-options.txt");
    let output = CommandRunner::new()
        .max_stdout_bytes(5)
        .tee_stdout_to_file(path.clone())
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc version command should run successfully");

    let file_bytes = fs::read(&path).expect("tee file should be readable");
    assert_eq!(output.stdout().len(), 5);
    assert!(file_bytes.starts_with(b"rustc "));
    assert!(file_bytes.len() > output.stdout().len());

    let _ = fs::remove_file(path);
}
