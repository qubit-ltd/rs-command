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
            Command::new("sh")
                .arg("-c")
                .arg("printf stdout; printf stderr >&2"),
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
    assert_eq!(
        std::fs::read(directory.path().join("stdout.log")).unwrap(),
        b"stdout"
    );
    assert_eq!(
        std::fs::read(directory.path().join("stderr.log")).unwrap(),
        b"stderr"
    );
}
