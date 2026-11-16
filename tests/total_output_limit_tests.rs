// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
mod support;

use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunner;
use support::total_output_process;
use support::total_output_process::Stream;

fn command(script: &str) -> Command {
    #[cfg(unix)]
    {
        Command::new("sh").arg("-c").arg(script)
    }
    #[cfg(windows)]
    {
        Command::new("cmd").arg("/C").arg(script)
    }
}

#[test]
fn test_total_output_empty_at_zero_limit_succeeds() {
    let cmd = {
        #[cfg(unix)]
        {
            command(":")
        }
        #[cfg(windows)]
        {
            command("exit /b 0")
        }
    };
    let output = CommandRunner::without_timeout()
        .max_total_output_bytes(0)
        .run(cmd)
        .expect("a command with no output fits a zero-byte budget");
    assert!(output.stdout().is_empty());
    assert!(output.stderr().is_empty());
}

#[test]
fn test_total_output_combines_both_streams_and_retains_sentinel() {
    let cmd = {
        #[cfg(unix)]
        {
            command("printf 12345; printf abcde >&2")
        }
        #[cfg(windows)]
        {
            command("<nul set /p =12345 & echo abcde 1>&2")
        }
    };
    let error = CommandRunner::without_timeout()
        .max_total_output_bytes(8)
        .run(cmd)
        .expect_err("stdout and stderr together exceed the hard budget");
    assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
    assert!(matches!(
        error.reason(),
        CommandErrorReason::OutputLimitExceeded { limit: 8, observed: 9 }
    ));
    assert!(error.output().is_none(), "budget errors must not invent an ExitStatus");
    let captured = error
        .captured_output()
        .expect("partial captured bytes should remain available");
    assert_eq!(captured.stdout().len() + captured.stderr().len(), 9);
    assert!(!captured.stdout_complete() || !captured.stderr_complete());
}

#[test]
fn test_total_output_exact_combined_limit_succeeds() {
    let cmd = {
        #[cfg(unix)]
        {
            command("printf 1234; printf abcd >&2")
        }
        #[cfg(windows)]
        {
            Command::new("powershell")
                .arg("-NoProfile")
                .arg("-Command")
                .arg("[Console]::OpenStandardOutput().Write([byte[]](49,50,51,52),0,4); [Console]::OpenStandardError().Write([byte[]](97,98,99,100),0,4)")
        }
    };
    let output = CommandRunner::without_timeout()
        .max_total_output_bytes(8)
        .run(cmd)
        .expect("exactly the configured combined byte count is allowed");
    assert_eq!(output.stdout().len() + output.stderr().len(), 8);
}

#[test]
fn test_total_output_idle_stream_does_not_reserve_budget() {
    let cmd = {
        #[cfg(unix)]
        {
            command("printf 123456789")
        }
        #[cfg(windows)]
        {
            command("<nul set /p =123456789")
        }
    };
    let error = CommandRunner::without_timeout()
        .max_total_output_bytes(8)
        .run(cmd)
        .expect_err("the active stdout stream must be charged when stderr is idle");
    assert!(matches!(
        error.reason(),
        CommandErrorReason::OutputLimitExceeded { limit: 8, observed: 9 }
    ));
}

#[test]
fn test_total_output_counts_bytes_discarded_by_memory_retention_limit() {
    let cmd = {
        #[cfg(unix)]
        {
            command("printf 123456789")
        }
        #[cfg(windows)]
        {
            command("<nul set /p =123456789")
        }
    };
    let error = CommandRunner::without_timeout()
        .max_total_output_bytes(8)
        .unbounded_output()
        .max_output_bytes(0)
        .run(cmd)
        .expect_err("the hard budget counts bytes independently of memory retention");
    let captured = error.captured_output().expect("budget error retains capture metadata");
    assert!(captured.stdout().is_empty());
    assert!(captured.stdout_truncated());
    assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
}

#[cfg(unix)]
#[test]
fn test_total_output_watches_inherited_pipe_after_direct_child_exit() {
    let error = CommandRunner::without_timeout()
        .max_total_output_bytes(8)
        .run(command("(sleep 0.02; printf 123456789) & exit 0"))
        .expect_err("descendant output after direct-child exit still counts");
    assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
}

#[test]
fn test_total_output_limit_configuration_is_opt_in_and_clone_local() {
    let runner = CommandRunner::without_timeout();
    assert_eq!(runner.configured_max_total_output_bytes(), None);
    let limited = runner.clone().max_total_output_bytes(u64::MAX);
    assert_eq!(limited.configured_max_total_output_bytes(), Some(u64::MAX));
    assert_eq!(runner.configured_max_total_output_bytes(), None);
    assert_eq!(
        limited
            .clone()
            .without_total_output_limit()
            .configured_max_total_output_bytes(),
        None
    );
}

#[test]
fn test_total_output_limit_does_not_change_nonzero_exit_classification() {
    let cmd = {
        #[cfg(unix)]
        {
            command("printf x; exit 7")
        }
        #[cfg(windows)]
        {
            command("echo x & exit /b 7")
        }
    };
    let error = CommandRunner::without_timeout()
        .max_total_output_bytes(10)
        .run(cmd)
        .expect_err("nonzero exit remains a process status error");
    assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
    assert_eq!(error.exit_code(), Some(7));
}
#[test]
fn test_total_output_limit_terminates_live_tree_for_stdout_budget_alone() {
    total_output_process::run(
        "test_total_output_limit_terminates_live_tree_for_stdout_budget_alone",
        Stream::Stdout,
    );
}

#[test]
fn test_total_output_supervisor_failure_cleanup() {
    total_output_process::test_supervisor_failures("test_total_output_supervisor_failure_cleanup");
}

#[test]
fn test_total_output_limit_terminates_live_tree_for_stderr_budget_alone() {
    total_output_process::run(
        "test_total_output_limit_terminates_live_tree_for_stderr_budget_alone",
        Stream::Stderr,
    );
}
