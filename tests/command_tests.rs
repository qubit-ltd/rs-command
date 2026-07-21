// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`Command`](qubit_command::Command).

#[cfg(unix)]
use std::{
    ffi::OsString,
    os::unix::ffi::OsStringExt,
};

use qubit_command::Command;

#[test]
fn test_command_new_stores_program() {
    let command = Command::new("git");

    assert_eq!(command.program().to_string_lossy(), "git");
    assert!(command.arguments().next().is_none());
}

#[test]
fn test_command_args_appends_in_order() {
    let command = Command::new("git")
        .arg("status")
        .args(&["--short", "--branch"]);

    let args = command
        .arguments()
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
fn test_command_debug_redacts_sensitive_display_values() {
    let command = Command::new("docker")
        .arg("login")
        .arg("--password")
        .arg("secret")
        .env("OPENAI_API_KEY", "abcdef")
        .stdin_bytes(b"stdin-secret".to_vec());

    let debug = format!("{command:?}");

    assert!(
        debug.contains(
            r#"argv: ["docker", "login", "--password", "<redacted>"]"#
        )
    );
    assert!(debug.contains(r#"env: ["OPENAI_API_KEY=****"]"#));
    assert!(debug.contains("stdin: Bytes(12 bytes)"));
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("abcdef"));
    assert!(!debug.contains("stdin-secret"));
}

#[test]
fn test_command_debug_masks_sensitive_option_after_double_dash() {
    let command = Command::new("wrapper").args(&[
        "--",
        "child",
        "--password",
        "raw-secret",
    ]);

    let debug = format!("{command:?}");

    assert!(!debug.contains("raw-secret"));
    assert!(debug.contains(r#"--password", "<redacted>"#));
}

#[test]
fn test_command_shell_payload_and_explicit_sensitive_argument_never_leak() {
    let shell = format!("{:?}", Command::shell("echo raw-shell-secret"));
    let explicit =
        format!("{:?}", Command::new("tool").sensitive_arg("raw-arg-secret"));

    assert!(!shell.contains("raw-shell-secret"));
    assert!(!explicit.contains("raw-arg-secret"));
}

#[cfg(unix)]
#[test]
fn test_command_debug_fails_closed_for_non_utf8_argument_and_environment() {
    let argument = OsString::from_vec(b"argument-secret-\xFF-suffix".to_vec());
    let environment =
        OsString::from_vec(b"environment-secret-\xFF-suffix".to_vec());
    let command = Command::new("tool")
        .arg_os(&argument)
        .env_os("MODE", &environment);

    let debug = format!("{command:?}");

    assert!(!debug.contains("argument-secret"));
    assert!(!debug.contains("environment-secret"));
    assert!(!debug.contains("suffix"));
}

#[test]
fn test_command_debug_redacts_credential_containers() {
    let command = Command::new("worker")
        .arg("--redis-url")
        .arg("redis://:argv-password@example.com")
        .env(
            "HTTPS_PROXY",
            "http://proxy-user:proxy-password@example.com",
        )
        .env("DOCKER_AUTH_CONFIG", r#"{"auths":{"secret":"value"}}"#);

    let debug = format!("{command:?}");

    assert!(!debug.contains("argv-password"));
    assert!(!debug.contains("proxy-password"));
    assert!(!debug.contains("\"secret\""));
    assert!(debug.contains("<redacted>"));
}

#[test]
fn test_command_debug_redacts_cmd_shell_payload() {
    let command = Command::new("cmd").arg("/C").arg("echo hunter2");

    let debug = format!("{command:?}");

    assert!(debug.contains(r#"argv: ["cmd", "/C", "<redacted>"]"#));
    assert!(!debug.contains("hunter2"));
}

#[test]
fn test_command_debug_formats_stdin_without_inline_bytes() {
    let null_input = format!("{:?}", Command::new("cat").stdin_null());
    let inherited_input = format!("{:?}", Command::new("cat").stdin_inherit());
    let file_input = format!(
        "{:?}",
        Command::new("cat")
            .working_directory("customer/working-directory")
            .stdin_file("customer/private-input.txt"),
    );

    assert!(null_input.contains("stdin: Null"));
    assert!(inherited_input.contains("stdin: Inherit"));
    assert!(file_input.contains(r#"stdin: File("<redacted path>")"#));
    assert!(
        file_input.contains("working_directory: Some(\"<redacted path>\")")
    );
    assert!(!file_input.contains("customer/working-directory"));
    assert!(!file_input.contains("customer/private-input.txt"));
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
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(args, vec!["-c", "printf ok"]);
}
