// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use qubit_command::Command;
use qubit_command::CommandRunner;

#[cfg(unix)]
#[test]
fn test_unix_output_and_stdin_guide_examples() {
    let runner = CommandRunner::new(std::time::Duration::from_secs(10));
    let output = runner
        .run(Command::new("printf").arg("hello"))
        .expect("printf should succeed");
    assert_eq!(output.stdout_text().expect("stdout should be UTF-8"), "hello");

    let output = runner
        .run(Command::new("cat").stdin_bytes("input\n").env("LANG", "C"))
        .expect("cat should echo stdin");
    assert_eq!(output.stdout_text().expect("stdout should be UTF-8"), "input\n");
}

#[cfg(unix)]
#[test]
fn test_unix_shell_guide_example() {
    let output = CommandRunner::new(std::time::Duration::from_secs(10))
        .run(Command::shell("printf hello | tr a-z A-Z"))
        .expect("the Unix shell pipeline should succeed");
    assert_eq!(output.stdout_text().expect("stdout should be UTF-8"), "HELLO");
}

#[cfg(windows)]
#[test]
fn test_windows_powershell_output_and_stdin_guide_examples() {
    let runner = CommandRunner::new(std::time::Duration::from_secs(10));
    let output = runner
        .run(Command::new("powershell").args(&["-NoProfile", "-Command", "[Console]::Write('hello')"]))
        .expect("PowerShell should write hello");
    assert_eq!(output.stdout_text().expect("stdout should be UTF-8"), "hello");

    let output = runner
        .run(
            Command::new("powershell")
                .args(&[
                    "-NoProfile",
                    "-Command",
                    "[Console]::Out.Write([Console]::In.ReadToEnd())",
                ])
                .stdin_bytes("input\n"),
        )
        .expect("PowerShell should echo stdin");
    assert_eq!(output.stdout_text().expect("stdout should be UTF-8"), "input\n");
}

#[cfg(windows)]
#[test]
fn test_windows_cmd_shell_guide_example() {
    let output = CommandRunner::new(std::time::Duration::from_secs(10))
        .run(Command::shell("echo hello"))
        .expect("cmd should echo hello");
    assert_eq!(output.stdout_text().expect("stdout should be UTF-8").trim(), "hello");
}
