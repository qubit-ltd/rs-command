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
fn test_managed_backend_writes_to_absolute_tee_paths() {
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
    let stdout_path = directory.path().join("stdout.log");
    let stderr_path = directory.path().join("stderr.log");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut operation = executor
        .submit(
            CommandRunner::new(Duration::from_secs(5)).working_directory(directory.path()),
            Command::new("sh").arg("-c").arg("printf stdout; printf stderr >&2"),
            CommandRunOptions::new()
                .tee_stdout_to_file(&stdout_path)
                .tee_stderr_to_file(&stderr_path),
        )
        .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => {
            assert_eq!(output.stdout(), b"stdout");
            assert_eq!(output.stderr(), b"stderr");
        }
        other => panic!("managed command failed: {other:?}"),
    }
    assert_eq!(std::fs::read(stdout_path).unwrap(), b"stdout");
    assert_eq!(std::fs::read(stderr_path).unwrap(), b"stderr");
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
    let script = format!("printf partial; sleep 30 & echo ready > '{}' ; wait", ready.display());
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
        assert!(std::time::Instant::now() < deadline, "child process should start");
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
    let diagnostic = operation
        .take_stop_diagnostic()
        .expect("settled stopped command should retain its runner diagnostics");
    assert_eq!(diagnostic.kind(), qubit_command::CommandErrorKind::Cancelled);
    assert_eq!(diagnostic.captured_output().unwrap().stdout(), b"partial");
    assert!(settlement.all_released());
    assert_eq!(
        settlement.external_settlement.as_ref().unwrap().status,
        SettlementStatus::Released
    );
}

#[cfg(unix)]
#[test]
fn test_external_cancel_after_child_exit_waits_for_helpers_and_retains_diagnostic() {
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandCancellation;
    use qubit_command::CommandExecutionConfig;
    use qubit_command::CommandExecutor;
    use qubit_command::CommandRunOptions;
    use qubit_command::CommandRunner;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::SettlementStatus;
    use qubit_operation::StopReason;

    let directory = tempfile::tempdir().unwrap();
    let ready_path = directory.path().join("helper-ready");
    let helper_pid_path = directory.path().join("helper.pid");
    let parent_pid_path = directory.path().join("parent.pid");
    let script = format!(
        "printf partial; sleep 30 & echo $! > '{}'; echo $$ > '{}'; touch '{}'; exit 0",
        helper_pid_path.display(),
        parent_pid_path.display(),
        ready_path.display(),
    );
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let cancellation = CommandCancellation::new();
    let mut operation = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg(&script),
            CommandRunOptions::new().cancellation(cancellation.clone()),
        )
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !ready_path.exists() {
        assert!(std::time::Instant::now() < deadline, "child helper should start");
        std::thread::sleep(Duration::from_millis(5));
    }
    let parent_pid: libc::pid_t = std::fs::read_to_string(&parent_pid_path).unwrap().trim().parse().unwrap();
    let helper_pid: libc::pid_t = std::fs::read_to_string(&helper_pid_path).unwrap().trim().parse().unwrap();
    while unsafe { libc::kill(parent_pid, 0) } == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "direct child should exit while inherited helper output remains pending"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        unsafe { libc::kill(helper_pid, 0) },
        0,
        "background helper should still hold inherited output open"
    );

    cancellation.cancel();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Stopped(report) => assert_eq!(report.reason, StopReason::UserCancelled),
        other => panic!("helper-pending managed command should stop: {other:?}"),
    }
    let settlement = operation.wait_settlement().unwrap();
    let diagnostic = operation
        .take_stop_diagnostic()
        .expect("typed stop should retain runner diagnostics");
    assert_eq!(diagnostic.kind(), qubit_command::CommandErrorKind::Cancelled);
    assert_eq!(diagnostic.captured_output().unwrap().stdout(), b"partial");
    assert!(settlement.all_released());
    assert_eq!(
        settlement.external_settlement.as_ref().unwrap().status,
        SettlementStatus::Released
    );
}
