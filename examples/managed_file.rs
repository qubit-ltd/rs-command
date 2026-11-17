// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::OpenOptions;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunner;
use qubit_command::FileSinkPolicy;
use qubit_command::ManagedCommandRunnerApi;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;

pub fn run() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let input_path = directory.path().join("input.txt");
    let output_path = directory.path().join("output.txt");
    std::fs::write(&input_path, b"skip-data").expect("seed input");
    std::fs::write(&output_path, b"old contents").expect("seed output");

    let mut input = OpenOptions::new()
        .read(true)
        .open(&input_path)
        .expect("open input");
    input.seek(SeekFrom::Start(5)).expect("set caller cursor");
    let position = input.stream_position().expect("read current position");
    assert_eq!(position, 5);

    let sink = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&output_path)
        .expect("open sink");
    let resources = CommandResourceRunOptions::new()
        .stdin_file(input)
        .stdout_sink(sink, FileSinkPolicy::Truncate);
    let command = if cfg!(windows) {
        Command::new("cmd").args(&["/C", "more"])
    } else {
        Command::new("cat")
    };
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let executor =
        CommandExecutor::new(manager, CommandExecutionConfig::default()).expect("executor");
    let output = CommandRunner::without_timeout()
        .run_managed_resources(&executor, command, resources)
        .expect("managed file command");
    assert_eq!(output.exit_code(), Some(0));

    // The run has returned after observing settlement, so the owned file input
    // is no longer in use. The caller still owns ordinary external cleanup of
    // its directory; the path string itself did not act as a lease.
    let written = std::fs::read(&output_path).expect("read sink");
    assert!(written.starts_with(b"data"));
    println!(
        "input started at byte {position}; sink now contains {:?}",
        written
    );
    drop(directory);
}

#[allow(dead_code)]
fn main() {
    run();
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_managed_file_example_runs() {
        super::run();
    }
}
