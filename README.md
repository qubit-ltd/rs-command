# Qubit Command

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-command.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-command)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-command/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-command?branch=main)
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
- Configurable timeout, working directory, environment variables, and success
  exit codes
- UTF-8 stdout and stderr text accessors, with raw byte accessors for binary
  output
- Typed errors for spawn failures, timeouts, failed output reads, and unexpected
  exit codes

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
UTF-8 bytes with `�`, enable lossy output on the runner:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .lossy_output(true)
    .run(Command::shell("printf '\\377'"))?;

assert_eq!(output.stdout()?, "\u{fffd}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Testing

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
