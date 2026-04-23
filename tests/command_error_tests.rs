/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for [`CommandError`](qubit_command::CommandError).

#![cfg(not(windows))]

use std::{
    io,
    time::Duration,
};

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
    OutputStream,
};

#[test]
fn test_command_error_accessors_for_errors_without_output() {
    let spawn = CommandError::SpawnFailed {
        command: "missing".to_owned(),
        source: io::Error::new(io::ErrorKind::NotFound, "missing"),
    };
    assert_eq!(spawn.command(), "missing");
    assert!(spawn.output().is_none());
    assert!(spawn.to_string().contains("failed to spawn command"));

    let wait = CommandError::WaitFailed {
        command: "wait".to_owned(),
        source: io::Error::other("wait failed"),
    };
    assert_eq!(wait.command(), "wait");
    assert!(wait.output().is_none());
    assert!(wait.to_string().contains("failed to wait"));

    let kill = CommandError::KillFailed {
        command: "kill".to_owned(),
        timeout: Duration::from_secs(1),
        source: io::Error::other("kill failed"),
    };
    assert_eq!(kill.command(), "kill");
    assert!(kill.output().is_none());
    assert!(kill.to_string().contains("failed to kill"));

    let read = CommandError::ReadOutputFailed {
        command: "read".to_owned(),
        stream: OutputStream::Stdout,
        source: io::Error::other("read failed"),
    };
    assert_eq!(read.command(), "read");
    assert!(read.output().is_none());
    assert!(read.to_string().contains("failed to read stdout"));
}

#[test]
fn test_command_error_accessors_for_errors_with_output() {
    let unexpected = CommandRunner::new()
        .run(Command::shell("printf output; exit 9"))
        .expect_err("non-success exit code should be rejected");
    assert!(unexpected.command().contains("exit 9"));
    assert_eq!(
        unexpected
            .output()
            .expect("unexpected exit should expose output")
            .stdout()
            .expect("stdout should be valid UTF-8"),
        "output",
    );

    let timed_out = CommandRunner::new()
        .timeout(Duration::from_millis(50))
        .run(Command::shell("printf before-timeout; sleep 2"))
        .expect_err("long-running command should time out");
    assert!(timed_out.command().contains("sleep 2"));
    assert_eq!(
        timed_out
            .output()
            .expect("timeout should expose captured output")
            .stdout()
            .expect("stdout should be valid UTF-8"),
        "before-timeout",
    );
}
