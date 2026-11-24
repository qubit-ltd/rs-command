// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandCancellation`](qubit_command::CommandCancellation).

#[cfg(unix)]
use qubit_command::Command;
use qubit_command::CommandCancellation;
#[cfg(unix)]
use qubit_command::CommandErrorKind;
#[cfg(unix)]
use qubit_command::CommandRunOptions;
#[cfg(unix)]
use qubit_command::CommandRunner;
#[cfg(unix)]
use std::time::Duration;

#[test]
fn test_command_cancellation_is_shared_and_one_shot() {
    let cancellation = CommandCancellation::new();
    let shared = cancellation.clone();

    assert!(!cancellation.is_cancelled());
    shared.cancel();

    assert!(cancellation.is_cancelled());
    cancellation.cancel();
    assert!(shared.is_cancelled());
}

#[cfg(unix)]
#[test]
fn test_legacy_runner_shared_token_broadcasts_to_each_run() {
    let directory = tempfile::tempdir().unwrap();
    let first_ready = directory.path().join("legacy-first-ready");
    let second_ready = directory.path().join("legacy-second-ready");
    let cancellation = CommandCancellation::new();
    let run = |ready: std::path::PathBuf, token: CommandCancellation| {
        std::thread::spawn(move || {
            CommandRunner::without_timeout().run_with(
                Command::new("sh")
                    .arg("-c")
                    .arg("touch \"$1\"; exec sleep 30")
                    .arg("legacy")
                    .arg(ready.to_str().unwrap()),
                CommandRunOptions::new().cancellation(token),
            )
        })
    };
    let first = run(first_ready.clone(), cancellation.clone());
    let second = run(second_ready.clone(), cancellation.clone());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while (!first_ready.exists() || !second_ready.exists()) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(first_ready.exists(), "first legacy command should start");
    assert!(second_ready.exists(), "second legacy command should start");

    cancellation.cancel();

    assert_eq!(first.join().unwrap().unwrap_err().kind(), CommandErrorKind::Cancelled);
    assert_eq!(second.join().unwrap().unwrap_err().kind(), CommandErrorKind::Cancelled);
}
