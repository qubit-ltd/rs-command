// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(unix)]

use std::io::Seek;
use std::io::Write;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunner;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;

#[test]
fn test_stdin_file_is_consumed_before_manager_reports_resource_cleanup() {
    let mut stdin = tempfile::tempfile().expect("stdin fixture should be created");
    stdin
        .write_all(b"handoff payload\n")
        .expect("stdin fixture should be written");
    stdin.rewind().expect("stdin should be rewound");

    let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).expect("executor should start");
    let mut operation = executor
        .submit_resources(
            CommandRunner::new(Duration::from_secs(5)),
            Command::new("cat"),
            CommandResourceRunOptions::new().stdin_file(stdin),
        )
        .expect("command should be accepted");

    let output = match operation.wait_outcome().expect("command outcome should be ready") {
        OperationOutcome::Succeeded(output) => output,
        other => panic!("stdin consumer did not succeed: {other:?}"),
    };
    assert_eq!(output.stdout(), b"handoff payload\n");

    let settlement = operation.wait_settlement().expect("manager settlement should be ready");
    assert!(settlement.all_released());
    assert_eq!(settlement.resource_reports_total, 1);
    assert_eq!(settlement.resource_reports.len(), 1);
}
