// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Behavior tests for [`CommandRunner`](qubit_command::CommandRunner)
//! configuration.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use qubit_clock::ManualMonotonicClock;
use qubit_clock::MonotonicClock;
use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;
use qubit_redact::RedactionPolicy;
use qubit_redact::Redactor;
use qubit_redact::Sensitivity;

#[test]
fn test_runner_constructors_expose_default_and_timeout_configuration() {
    let timed = CommandRunner::new(Duration::from_secs(12));
    assert_eq!(timed.configured_timeout(), Some(Duration::from_secs(12)));
    assert_eq!(timed.configured_success_exit_codes(), &[0]);
    assert!(timed.configured_working_directory().is_none());
    assert!(!timed.is_logging_disabled());
    assert!(timed.is_output_truncation_failure_enabled());
    assert_eq!(
        timed.configured_diagnostic_redaction_policy(),
        Redactor::application_default().policy(),
    );
    assert_eq!(
        timed.configured_max_stdout_bytes(),
        Some(qubit_command::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
    );
    assert_eq!(
        timed.configured_max_stderr_bytes(),
        Some(qubit_command::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
    );
    assert_eq!(timed.configured_max_total_output_bytes(), None);

    let untimed = CommandRunner::without_timeout();
    assert_eq!(untimed.configured_timeout(), None);
    assert_eq!(untimed.configured_success_exit_codes(), &[0]);
}

#[test]
fn test_runner_working_directory_and_timer_builders_are_observable() {
    let directory = PathBuf::from("configured/child-directory");
    let clock = ManualMonotonicClock::new_shared();
    let expected_domain = clock.now().domain();
    let runner = CommandRunner::without_timeout()
        .working_directory(directory.clone())
        .timer(Arc::new(clock.new_timer()));

    assert_eq!(runner.configured_working_directory(), Some(directory.as_path()));
    assert_eq!(runner.configured_timer().clock().now().domain(), expected_domain);
}

#[cfg(unix)]
fn exit_7_command() -> Command {
    Command::new("sh").arg("-c").arg("exit 7")
}

#[cfg(windows)]
fn exit_7_command() -> Command {
    Command::new("cmd").arg("/C").arg("exit 7")
}

#[cfg(unix)]
fn print_working_directory_command() -> Command {
    Command::new("sh").arg("-c").arg("pwd")
}

#[cfg(windows)]
fn print_working_directory_command() -> Command {
    Command::new("cmd").arg("/C").arg("cd")
}

#[test]
fn test_runner_accepts_owned_os_string_working_directory_in_sync_run() {
    let directory = tempfile::tempdir().expect("working directory should be created");
    let working_directory: OsString = directory.path().as_os_str().to_os_string();
    let runner = CommandRunner::without_timeout().working_directory(working_directory);

    assert_eq!(runner.configured_working_directory(), Some(directory.path()));
    let output = runner
        .run(print_working_directory_command())
        .expect("synchronous run should use the configured owned working directory");
    let expected_working_directory = directory
        .path()
        .canonicalize()
        .expect("working directory should resolve to an absolute path");
    let actual_working_directory = std::path::PathBuf::from(String::from_utf8_lossy(output.stdout()).trim())
        .canonicalize()
        .expect("reported working directory should resolve to an absolute path");
    assert_eq!(actual_working_directory, expected_working_directory);
}

#[test]
fn test_runner_success_code_builders_replace_the_accepted_set() {
    let accepted = CommandRunner::without_timeout().success_exit_codes(&[3, 7]);
    assert_eq!(accepted.configured_success_exit_codes(), &[3, 7]);
    assert_eq!(
        accepted
            .run(exit_7_command())
            .expect("a listed exit status is accepted")
            .exit_code(),
        Some(7),
    );

    let replaced = CommandRunner::without_timeout()
        .success_exit_codes(&[3, 7])
        .success_exit_code(3);
    assert_eq!(replaced.configured_success_exit_codes(), &[3]);
    let error = replaced
        .run(exit_7_command())
        .expect_err("success_exit_code replaces the prior accepted set");
    assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
}

#[test]
fn test_runner_output_configuration_builders_keep_independent_policies() {
    let per_stream = CommandRunner::without_timeout()
        .max_stdout_bytes(2)
        .max_stderr_bytes(4)
        .max_total_output_bytes(9);
    assert_eq!(per_stream.configured_max_stdout_bytes(), Some(2));
    assert_eq!(per_stream.configured_max_stderr_bytes(), Some(4));
    assert_eq!(per_stream.configured_max_total_output_bytes(), Some(9));

    let shared = per_stream.clone().max_output_bytes(6);
    assert_eq!(shared.configured_max_stdout_bytes(), Some(6));
    assert_eq!(shared.configured_max_stderr_bytes(), Some(6));
    assert_eq!(shared.configured_max_total_output_bytes(), Some(9));

    let bounded = shared.clone().fail_on_output_truncation(false).bounded_output(5);
    assert_eq!(bounded.configured_max_stdout_bytes(), Some(5));
    assert_eq!(bounded.configured_max_stderr_bytes(), Some(5));
    assert!(bounded.is_output_truncation_failure_enabled());
    assert!(bounded.configured_max_total_output_bytes().is_some());

    let unbounded = bounded.unbounded_output();
    assert_eq!(unbounded.configured_max_stdout_bytes(), None);
    assert_eq!(unbounded.configured_max_stderr_bytes(), None);
    assert!(!unbounded.is_output_truncation_failure_enabled());
    assert_eq!(unbounded.configured_max_total_output_bytes(), Some(9));

    let no_total_limit = unbounded.without_total_output_limit();
    assert_eq!(no_total_limit.configured_max_total_output_bytes(), None);
    assert_eq!(no_total_limit.configured_max_stdout_bytes(), None);
    assert_eq!(no_total_limit.configured_max_stderr_bytes(), None);
}

#[test]
fn test_runner_logging_truncation_and_redaction_builders_round_trip() {
    let policy = RedactionPolicy::default()
        .to_builder()
        .fields(|fields| {
            fields.raise("customer_token", Sensitivity::Secret);
        })
        .expect("the custom field name should be valid")
        .build()
        .expect("the configured redaction policy should be valid");
    let runner = CommandRunner::new(Duration::from_secs(1))
        .disable_logging(true)
        .fail_on_output_truncation(false)
        .diagnostic_redaction_policy(policy.clone());

    assert!(runner.is_logging_disabled());
    assert!(!runner.is_output_truncation_failure_enabled());
    assert_eq!(runner.configured_diagnostic_redaction_policy(), &policy);
    assert!(!runner.clone().disable_logging(false).is_logging_disabled());
    assert!(
        runner
            .clone()
            .fail_on_output_truncation(true)
            .is_output_truncation_failure_enabled()
    );
}

#[cfg(unix)]
#[test]
fn test_runner_configuration_composes_for_nonzero_exit_and_independent_capture_limits() {
    let runner = CommandRunner::without_timeout()
        .success_exit_codes(&[3, 7])
        .max_stdout_bytes(3)
        .max_stderr_bytes(2)
        .fail_on_output_truncation(false)
        .disable_logging(true);

    assert_eq!(runner.configured_timeout(), None);
    assert_eq!(runner.configured_success_exit_codes(), &[3, 7]);
    assert_eq!(runner.configured_max_stdout_bytes(), Some(3));
    assert_eq!(runner.configured_max_stderr_bytes(), Some(2));
    assert!(!runner.is_output_truncation_failure_enabled());
    assert!(runner.is_logging_disabled());

    let output = runner
        .run(
            Command::new("sh")
                .arg("-c")
                .arg("printf 'stdout'; printf 'stderr' >&2; exit 7"),
        )
        .expect("listed nonzero exit code should succeed even with truncated capture");
    assert_eq!(output.exit_code(), Some(7));
    assert_eq!(output.stdout(), b"std");
    assert_eq!(output.stderr(), b"st");
    assert!(output.stdout_truncated());
    assert!(output.stderr_truncated());
}
