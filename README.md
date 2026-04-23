# Qubit Command

[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

Command-line process running utilities for Qubit Rust projects.

## Overview

Qubit Command provides a small, structured API for running external programs,
capturing their output, enforcing timeouts, and reporting command failures with
clear error values.

This crate is intentionally separate from `qubit-concurrent`: running an
external command is process management, not a generic task execution strategy.

## Features

- Structured command execution with program and argument vectors
- Explicit shell command support for cases that require shell parsing
- Configurable timeout, working directory, environment variables, and success
  exit codes
- Captured stdout and stderr as raw bytes, with UTF-8 helper methods
- Typed errors for spawn failures, timeouts, failed output reads, and unexpected
  exit codes

## Quick Start

```rust
use std::time::Duration;

use qubit_command::{CommandRunner, CommandSpec};

let output = CommandRunner::new()
    .timeout(Duration::from_secs(10))
    .run(CommandSpec::new("git").args(&["status", "--short"]))?;

println!("{}", output.stdout_utf8()?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Shell Commands

Prefer structured commands whenever possible:

```rust
use qubit_command::{CommandRunner, CommandSpec};

let output = CommandRunner::new()
    .run(CommandSpec::new("printf").arg("hello"))?;

assert_eq!(output.stdout_utf8()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `CommandSpec::shell` only when shell parsing, redirection, expansion, or
pipes are intentional:

```rust
use qubit_command::{CommandRunner, CommandSpec};

let output = CommandRunner::new()
    .run(CommandSpec::shell("printf hello | tr a-z A-Z"))?;

assert_eq!(output.stdout_utf8()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Testing

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
