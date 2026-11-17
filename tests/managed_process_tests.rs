// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[test]
fn test_managed_process_backend_contract_is_available() {
    use qubit_command::Command;
    use qubit_command::CommandRunner;
    let output = CommandRunner::without_timeout()
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc should run");
    assert!(!output.stdout().is_empty());
}

#[cfg(unix)]
#[test]
fn test_managed_backend_resolves_relative_tee_paths_from_runner_directory() {
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandExecutionConfig;
    use qubit_command::CommandExecutor;
    use qubit_command::CommandRunOptions;
    use qubit_command::CommandRunner;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut operation = executor
        .submit(
            CommandRunner::new(Duration::from_secs(5)).working_directory(directory.path()),
            Command::new("sh").arg("-c").arg("printf stdout; printf stderr >&2"),
            CommandRunOptions::new()
                .tee_stdout_to_file("stdout.log")
                .tee_stderr_to_file("stderr.log"),
        )
        .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => {
            assert_eq!(output.stdout(), b"stdout");
            assert_eq!(output.stderr(), b"stderr");
        }
        other => panic!("managed command failed: {other:?}"),
    }
    assert_eq!(std::fs::read(directory.path().join("stdout.log")).unwrap(), b"stdout");
    assert_eq!(std::fs::read(directory.path().join("stderr.log")).unwrap(), b"stderr");
}

#[test]
fn test_managed_spawn_failure_publishes_failure_and_released_settlement() {
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandErrorReason;
    use qubit_command::CommandExecutionConfig;
    use qubit_command::CommandExecutor;
    use qubit_command::CommandRunner;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::SettlementStatus;

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut operation = executor
        .submit(
            CommandRunner::new(Duration::from_secs(5)),
            Command::new("__qubit_command_missing_executable_for_managed_test__"),
            Default::default(),
        )
        .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(error) => {
            assert!(matches!(error.reason(), CommandErrorReason::SpawnFailed { .. }));
        }
        other => panic!("missing executable should fail: {other:?}"),
    }
    let settlement = operation.wait_settlement().unwrap();
    assert!(settlement.all_released());
    assert_eq!(
        settlement.external_settlement.as_ref().unwrap().status,
        SettlementStatus::Released
    );
}

#[cfg(unix)]
#[test]
fn test_managed_stop_after_child_start_waits_for_tree_settlement() {
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandExecutionConfig;
    use qubit_command::CommandExecutor;
    use qubit_command::CommandRunner;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::SettlementStatus;
    use qubit_operation::StopReason;
    use qubit_operation::StopRequestStatus;

    let directory = tempfile::tempdir().unwrap();
    let ready = directory.path().join("child-ready");
    let script = format!("sleep 30 & echo ready > '{}' ; wait", ready.display());
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut operation = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg(&script),
            Default::default(),
        )
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !ready.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "child process should start"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        operation.request_stop(StopReason::UserCancelled),
        StopRequestStatus::Accepted
    );
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Stopped(report) => assert_eq!(report.reason, StopReason::UserCancelled),
        other => panic!("running managed process should stop: {other:?}"),
    }
    let settlement = operation.wait_settlement().unwrap();
    assert!(settlement.all_released());
    assert_eq!(
        settlement.external_settlement.as_ref().unwrap().status,
        SettlementStatus::Released
    );
}
