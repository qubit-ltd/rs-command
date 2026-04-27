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
fn test_command_os_args_appends_in_order() {
    let command = Command::new_os(std::ffi::OsStr::new("git"))
        .arg_os(std::ffi::OsStr::new("status"))
        .args_os([std::ffi::OsStr::new("--short")]);

    let args = command
        .arguments()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(args, vec!["status", "--short"]);
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
fn test_command_env_replaces_existing_override() {
    let command = Command::new("env")
        .env("QUBIT_COMMAND_TEST", "before")
        .env("QUBIT_COMMAND_TEST", "after");

    let envs = command.environment();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0].0.to_string_lossy(), "QUBIT_COMMAND_TEST");
    assert_eq!(envs[0].1.to_string_lossy(), "after");
}

#[test]
fn test_command_env_os_removes_prior_removal() {
    let command = Command::new("env").env_remove("QUBIT_COMMAND_TEST").env_os(
        std::ffi::OsStr::new("QUBIT_COMMAND_TEST"),
        std::ffi::OsStr::new("present"),
    );

    assert!(command.removed_environment().is_empty());
    assert_eq!(command.environment().len(), 1);
    assert_eq!(
        command.environment()[0].0.to_string_lossy(),
        "QUBIT_COMMAND_TEST",
    );
}

#[test]
fn test_command_env_remove_records_removal() {
    let command = Command::new("env")
        .env("QUBIT_COMMAND_TEST", "present")
        .env_remove("QUBIT_COMMAND_TEST");

    assert!(command.environment().is_empty());
    assert_eq!(
        command.removed_environment()[0].to_string_lossy(),
        "QUBIT_COMMAND_TEST",
    );
}

#[test]
fn test_command_env_remove_deduplicates_removals() {
    let command = Command::new("env")
        .env_remove("QUBIT_COMMAND_TEST")
        .env_remove("QUBIT_COMMAND_TEST");

    assert_eq!(command.removed_environment().len(), 1);
    assert_eq!(
        command.removed_environment()[0].to_string_lossy(),
        "QUBIT_COMMAND_TEST",
    );
}

#[test]
#[cfg(not(windows))]
fn test_command_env_names_are_case_sensitive_on_unix() {
    let command = Command::new("env")
        .env("QUBIT_COMMAND_TEST", "upper")
        .env("qubit_command_test", "lower")
        .env_remove("QUBIT_COMMAND_TEST");

    assert_eq!(command.environment().len(), 1);
    assert_eq!(
        command.environment()[0].0.to_string_lossy(),
        "qubit_command_test",
    );
    assert_eq!(
        command.removed_environment()[0].to_string_lossy(),
        "QUBIT_COMMAND_TEST",
    );
}

#[test]
#[cfg(windows)]
fn test_command_env_names_are_case_insensitive_on_windows() {
    let command = Command::new("env")
        .env("QUBIT_COMMAND_TEST", "upper")
        .env("qubit_command_test", "lower")
        .env_remove("QUBIT_COMMAND_TEST");

    assert!(command.environment().is_empty());
    assert_eq!(
        command.removed_environment()[0].to_string_lossy(),
        "QUBIT_COMMAND_TEST",
    );
}

#[test]
#[cfg(windows)]
fn test_command_env_names_use_ordinal_case_insensitive_comparison_on_windows() {
    use std::os::windows::ffi::OsStringExt;

    let first_invalid_key = std::ffi::OsString::from_wide(&[0xD800]);
    let second_invalid_key = std::ffi::OsString::from_wide(&[0xD801]);
    let command = Command::new("env")
        .env_os(&first_invalid_key, "first")
        .env_remove_os(&second_invalid_key);

    assert_eq!(command.environment().len(), 1);
    assert_eq!(command.removed_environment().len(), 1);
}

#[test]
fn test_command_env_clear_clears_prior_environment_changes() {
    let command = Command::new("env")
        .env("QUBIT_COMMAND_TEST", "present")
        .env_remove("QUBIT_COMMAND_REMOVED")
        .env_clear();

    assert!(command.clears_environment());
    assert!(command.environment().is_empty());
    assert!(command.removed_environment().is_empty());
}

#[test]
fn test_command_stdin_null_is_configurable() {
    let command = Command::new("cat").stdin_null();

    assert_eq!(command.program().to_string_lossy(), "cat");
}

#[test]
fn test_command_stdin_inherit_is_configurable() {
    let command = Command::new("cat").stdin_inherit();

    assert_eq!(command.program().to_string_lossy(), "cat");
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
