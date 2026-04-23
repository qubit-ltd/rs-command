/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for [`CommandSpec`](qubit_command::CommandSpec).

use qubit_command::CommandSpec;

#[test]
fn test_command_spec_new_stores_program() {
    let spec = CommandSpec::new("git");

    assert_eq!(spec.program().to_string_lossy(), "git");
    assert!(spec.arguments().is_empty());
}

#[test]
fn test_command_spec_args_appends_in_order() {
    let spec = CommandSpec::new("git")
        .arg("status")
        .args(&["--short", "--branch"]);

    let args = spec
        .arguments()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(args, vec!["status", "--short", "--branch"]);
}

#[test]
fn test_command_spec_env_records_override() {
    let spec = CommandSpec::new("env").env("QUBIT_COMMAND_TEST", "present");

    let envs = spec.environment();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0].0.to_string_lossy(), "QUBIT_COMMAND_TEST");
    assert_eq!(envs[0].1.to_string_lossy(), "present");
}

#[test]
#[cfg(not(windows))]
fn test_command_spec_shell_uses_unix_shell() {
    let spec = CommandSpec::shell("printf ok");

    assert_eq!(spec.program().to_string_lossy(), "sh");
    let args = spec
        .arguments()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(args, vec!["-c", "printf ok"]);
}
