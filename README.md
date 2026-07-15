# Qubit Command

[![Rust CI](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-command/coverage-badge.json)](https://qubit-ltd.github.io/rs-command/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-command.svg?color=blue)](https://crates.io/crates/qubit-command)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

Command-line process running utilities for Rust.

## Overview

Qubit Command provides a small, structured API for running external programs,
capturing their output, enforcing timeouts, and reporting command failures with
clear error values.

## Features

- Structured command execution with program and argument vectors
- Explicit shell command support for cases that require shell parsing
- Configurable timeout, working directory, stdin, environment variables, and
  success exit codes
- Process-tree termination on timeout using Unix process groups and Windows Job
  Objects
- UTF-8 stdout and stderr text accessors, with raw byte accessors for binary
  output
- Optional per-stream capture limits plus streaming tee files for large output
- Sanitized command diagnostics for sensitive argv values, explicit
  environment overrides, shell payloads, and caller-defined sensitive fields
- Typed errors for spawn failures, timeouts, failed output reads, and unexpected
  exit codes

## Timeout Behavior

`CommandRunner::new()` does not enforce a timeout by default. Use
`timeout(Duration)` when a command must be bounded, or `without_timeout()` when
the absence of a timeout should be explicit in builder chains.

When a timeout is configured, the runner attempts to terminate the process tree:
Unix commands are spawned in a new process group and Windows commands are spawned
in a Job Object.

## Large Output

By default stdout and stderr are captured without an in-memory byte limit. For
commands that can emit large logs, configure capture limits and tee files:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .max_output_bytes(64 * 1024)
    .tee_stdout_to_file("stdout.log")
    .tee_stderr_to_file("stderr.log")
    .run(Command::new("cargo").arg("test"))?;

if output.stdout_truncated() {
    eprintln!("stdout was truncated in memory; see stdout.log for the full stream");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Quick Start

```rust
use std::time::Duration;

use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .timeout(Duration::from_secs(10))
    .run(Command::new("git").args(&["status", "--short"]))?;

println!("{}", output.stdout_text()?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Shell Commands

Prefer structured commands whenever possible:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::new("printf").arg("hello"))?;

assert_eq!(output.stdout_text()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `Command::shell` only when shell parsing, redirection, expansion, or
pipes are intentional:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::shell("printf hello | tr a-z A-Z"))?;

assert_eq!(output.stdout_text()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Sanitized Diagnostics

Command strings used in runner logs, `CommandError::command()`, and
`Command`'s `Debug` output are sanitized with `qubit-sanitize`.
Sensitive structured argv values such as `--password secret`,
`--access-token=...`, and `OPENAI_API_KEY=...` are masked. Explicit
environment overrides are shown only in sanitized form. `Command::shell`
payloads are treated as opaque shell scripts and displayed as
`<shell command>` instead of being parsed.

Add application-specific fields on the runner when the defaults are not enough:

```rust
use qubit_command::{Command, CommandRunner, SensitivityLevel};

let error = CommandRunner::new()
    .sensitive_field("tenant_option", SensitivityLevel::Secret)
    .run(Command::new("__missing__").arg("--tenant-option").arg("secret"))
    .expect_err("sample command should fail");

assert_eq!(
    error.command(),
    r#"["__missing__", "--tenant-option", "<redacted>"]"#,
);
```

Runner-specific fields affect runner logs and `CommandError::command()`.
Standalone `Command` `Debug` output has no runner context and uses the built-in
defaults only.

`CommandOutput`'s `Debug` output redacts both captured streams and reports only
their byte lengths, truncation flags, status, and elapsed time. Captured
stdout/stderr bytes, their explicit accessors, and tee files remain raw process
output. Use capture limits and caller-side filtering when command output itself
may contain secrets.

## Output Text

`stdout()` and `stderr()` return raw bytes exactly as retained. Use
`stdout_text()` and `stderr_text()` when the command output must be valid UTF-8.
Use `stdout_lossy_text()` and `stderr_lossy_text()` to replace invalid UTF-8
bytes with `�`.

If the captured stdout or stderr contains invalid UTF-8, `stdout_text()` /
`stderr_text()` return `Err(str::Utf8Error)` from `str::from_utf8`. The bytes are
still stored on the returned `CommandOutput`; use `stdout()` / `stderr()` to read
the raw output and decode or handle it yourself.

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::shell("printf '\\377'"))?;

assert_eq!(output.stdout_lossy_text(), "\u{fffd}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Testing

A minimal local run:

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

To mirror what continuous integration enforces, run the repository scripts from the project root: `./align-ci.sh` brings local tooling and configuration in line with CI, then `./ci-check.sh` runs the same checks the pipeline uses. For test coverage, use `./coverage.sh` to generate or open reports (see the script’s help and any project coverage notes for options such as HTML or JSON).

## Contributing

Issues and pull requests are welcome.

- Open an issue for bug reports, design questions, or larger feature proposals when it helps align on direction.
- Keep pull requests scoped to one behavior change, fix, or documentation update when practical.
- Before submitting, run `./align-ci.sh` and then `./ci-check.sh` so your branch matches CI rules and passes the same checks as the pipeline. When you need to review or improve coverage, use `./coverage.sh` as described under [Testing](#testing).
- Add or update tests when you change runtime behavior, and update this README (or public rustdoc) when user-visible API behavior changes.

By contributing, you agree to license your contributions under the [Apache License, Version 2.0](LICENSE), the same license as this project.

## License

Copyright © 2026 Haixing Hu, Qubit Co. Ltd.

This project is licensed under the [Apache License, Version 2.0](LICENSE). See the `LICENSE` file in the repository for the full text.

## Author

**Haixing Hu** — Qubit Co. Ltd.

| | |
| --- | --- |
| **Repository** | [github.com/qubit-ltd/rs-command](https://github.com/qubit-ltd/rs-command) |
| **Documentation** | [docs.rs/qubit-command](https://docs.rs/qubit-command) |
| **Crate** | [crates.io/crates/qubit-command](https://crates.io/crates/qubit-command) |
