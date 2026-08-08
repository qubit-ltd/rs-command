# Qubit Command

[![Rust CI](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-command/coverage-badge.json)](https://qubit-ltd.github.io/rs-command/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-command.svg?color=blue)](https://crates.io/crates/qubit-command)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

Qubit Command is a Rust library for running external programs when an application needs more than a bare `std::process::Command`: bounded output capture, a defined timeout or cancellation policy, process-tree cleanup, and typed failures with redacted diagnostics. It is intended for build tools, service workers, and other applications that must turn an external process into a controlled application step.

## Installation

The crate is published as `qubit-command` and requires Rust 1.94 or newer:

```toml
[dependencies]
qubit-command = "0.6"
```

## Quick Start

Suppose a service needs to run a repository check and include its output in a result. Build the command from the program and argument values, run it with the default policy, and decode stdout only after the command succeeds:

```rust
use qubit_command::{Command, CommandRunner};

use std::time::Duration;

fn repository_status() -> Result<String, Box<dyn std::error::Error>> {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("git").args(&["status", "--short"]))?;

    Ok(output.stdout_text()?.to_owned())
}
```

The structured form passes arguments without shell parsing. For an intentional shell pipeline or redirection, use `Command::shell(...)` explicitly and treat the shell command line as an input that requires the caller's own validation.

## Why This Project Exists

Launching a child process is easy; defining what happens when it hangs, produces too much output, is cancelled while descendants keep pipes open, exits with an unexpected status, or includes secrets in diagnostics is not. Qubit Command puts those decisions in `CommandRunner` and makes the result observable through `CommandOutput` and `CommandError`.

The library is useful when the caller needs a repeatable policy around an external process. It does not replace the target program's own argument parsing, provide a shell-language abstraction, or install a global signal handler.

## What It Provides

- `Command` describes a program, structured arguments, optional shell execution, working-directory and environment overrides, and stdin configuration.
- `CommandRunner` applies timeout, cancellation, successful exit-code, logging, output-capture, tee-file, and diagnostic-redaction policies.
- `CommandOutput` exposes exit status, raw stdout/stderr bytes, strict or lossy UTF-8 views, elapsed time, truncation flags, and stream-completion flags.
- `CommandError` distinguishes preparation, spawn, wait, output, timeout, cancellation, truncation, and unexpected-exit failures. Timeout, cancellation, truncation, unexpected-exit, tee-write, output-read, and final stdin-write errors retain `CommandOutput` when the process status and stream state can be assembled; preparation, thread-start, clock, and process-control errors may not.
- `CommandCancellation` is a one-shot handle for an application-owned shutdown or terminal-signal policy. The crate does not install signal handlers.
- When timeout or cancellation management is enabled, the runner attempts to terminate the process tree using a Unix process group or a Windows Job Object.
- Each output stream is limited to 1 MiB in memory by default. Tee files can retain the complete stream while the in-memory result stays bounded.
- Command diagnostics and lifecycle logs redact sensitive argument, environment, shell, and path values. Captured process output and tee files remain raw output and must be handled by the caller.

Important boundaries:

- `Command::new` avoids shell parsing, but the target executable still interprets its own options.
- `Command::shell` uses `sh -c` on Unix-like platforms and `cmd /C` on Windows; it is not a portable shell-language guarantee.
- `unbounded_output()` removes the in-memory capture limit and should only be used when the command's output is known to be finite and acceptable.
- Timeout and cancellation results may contain partial output. Check `stdout_complete()` and `stderr_complete()` before treating retained bytes as complete streams.
- `stdin_file`, `tee_stdout_to_file`, and `tee_stderr_to_file` accept ordinary files only. Conflicting input and output paths are rejected before tee files are truncated.
- On Unix, path-backed stdin and stdout/stderr tee files are opened with non-blocking safety flags, validated from the opened handle, and restored to blocking mode before use. This prevents a FIFO replacement from blocking command preparation. Other platforms validate the opened handle but cannot portably guarantee that every device-namespace open returns promptly; supply trusted ordinary-file paths there.
- Tee files are replaced (not appended) at the start of each run. `CommandRunOptions::clone()` copies tee paths; concurrent runs must use distinct paths when their logs must remain separate.

## Learn More

- [English user guide](doc/user_guide.md)
- [中文用户手册](doc/user_guide.zh_CN.md)
- [API documentation on docs.rs](https://docs.rs/qubit-command)
- [Command-runner I/O lifecycle design](doc/command-runner-io-lifecycle-design.md)
- [中文 README](README.zh_CN.md)

## Testing

```bash
# Run tests with the default feature set
cargo test

# Run tests with all declared features
cargo test --all-features

# Project CI checks
./ci-check.sh

# Check code coverage
./coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./align-ci.sh` to format code and
`./ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-command](https://github.com/qubit-ltd/rs-command)
