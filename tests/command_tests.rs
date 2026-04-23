/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for [`Command`](qubit_command::Command).

use qubit_command::Command;

#[test]
fn test_command_new_stores_program() {
    let command = Command::new("git");

    assert_eq!(command.program().to_string_lossy(), "git");
    assert!(command.arguments().is_empty());
}

#[test]
fn test_command_args_appends_in_order() {
    let command = Command::new("git")
        .arg("status")
        .args(&["--short", "--branch"]);

    let args = command
        .arguments()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(args, vec!["status", "--short", "--branch"]);
}

#[test]
fn test_command_env_records_override() {
    let command = Command::new("env").env("QUBIT_COMMAND_TEST", "present");

    let envs = command.environment();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0].0.to_string_lossy(), "QUBIT_COMMAND_TEST");
    assert_eq!(envs[0].1.to_string_lossy(), "present");
}

#[test]
#[cfg(not(windows))]
fn test_command_shell_uses_unix_shell() {
    let command = Command::shell("printf ok");

    assert_eq!(command.program().to_string_lossy(), "sh");
    let args = command
        .arguments()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(args, vec!["-c", "printf ok"]);
}
