# Qubit Command

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-command.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-command)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-command/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-command?branch=main)
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

println!("{}", output.stdout()?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Shell Commands

Prefer structured commands whenever possible:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::new("printf").arg("hello"))?;

assert_eq!(output.stdout()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `Command::shell` only when shell parsing, redirection, expansion, or
pipes are intentional:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::shell("printf hello | tr a-z A-Z"))?;

assert_eq!(output.stdout()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Output Text

`stdout()` and `stderr()` return UTF-8 text by default. Use `stdout_bytes()` and
`stderr_bytes()` when a command can emit arbitrary bytes. To replace invalid
UTF-8 bytes with `�`, enable lossy output on the runner.

If lossy output is disabled and the captured stdout or stderr contains invalid
UTF-8, `stdout()` / `stderr()` return `Err(str::Utf8Error)` from
`str::from_utf8`—you cannot obtain a `&str` for that stream. The bytes are still
stored on the returned `CommandOutput`; use `stdout_bytes()` / `stderr_bytes()` to read
the raw output and decode or handle it yourself.

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .lossy_output(true)
    .run(Command::shell("printf '\\377'"))?;

assert_eq!(output.stdout()?, "\u{fffd}");
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
