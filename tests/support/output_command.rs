// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Builds a command that emits exact raw bytes on both supported platforms.

use qubit_command::Command;

/// Creates a command that writes the supplied bytes without text conversion.
pub(crate) fn output_command(stdout: &[u8], stderr: &[u8], exit_code: i32) -> Command {
    #[cfg(unix)]
    {
        let stdout = printf_octal(stdout);
        let stderr = printf_octal(stderr);
        Command::shell(&format!("printf '{stdout}'; printf '{stderr}' >&2; exit {exit_code}"))
    }
    #[cfg(windows)]
    {
        let stdout = powershell_bytes(stdout);
        let stderr = powershell_bytes(stderr);
        let script = format!(
            "$stdout = [byte[]]@({stdout}); $stderr = [byte[]]@({stderr}); $out = [Console]::OpenStandardOutput(); $err = [Console]::OpenStandardError(); $out.Write($stdout, 0, $stdout.Length); $err.Write($stderr, 0, $stderr.Length); exit {exit_code}"
        );
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(&script)
    }
}

#[cfg(unix)]
fn printf_octal(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("\\{byte:03o}")).collect()
}

#[cfg(windows)]
fn powershell_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
}
