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
- Explicit one-shot cancellation handles for applications that manage terminal
  signals or shutdown requests
- Process-tree termination on timeout or cancellation using Unix process groups
  and Windows Job Objects
- UTF-8 stdout and stderr text accessors, with raw byte accessors for binary
  output
- Optional per-stream capture limits plus streaming tee files for large output
- Optional failure policy for successful commands whose captured output is
  truncated
- Input and output file conflict detection before any tee file is truncated
- Redacted command diagnostics for sensitive argv values, explicit
  environment overrides, shell payloads, and caller-defined sensitive fields
- Typed errors for spawn failures, timeouts, failed output reads, and unexpected
  exit codes

## Timeout Behavior

`CommandRunner::new()` enforces `DEFAULT_COMMAND_TIMEOUT` (currently ten
seconds). Use `timeout(Duration)` when a command needs a different bound, or
`without_timeout()` only when the absence of a timeout is deliberate.
The timeout clock starts after the child process has been spawned; time spent
preparing and spawning a command is not included. Preparation opens configured
stdin and tee paths. Opening a FIFO, device, or other special file may block
until an external peer or device becomes ready, independently of the command
timeout.

Each polling step checks the direct child before checking the deadline. After
an observed child exit, output collection remains bounded by the same timeout.
Reaching the timeout starts process-tree termination and cleanup; it is not a
hard upper bound on when `run()` returns. Platform termination and I/O helper
cleanup can take additional time. A descendant that escapes the managed Unix
process group or Windows Job Object while retaining an inherited I/O pipe
can delay return until that pipe closes.

When a timeout or cancellation handle is configured, the runner attempts to
terminate the process tree: Unix commands are spawned in a new process group
and Windows commands are spawned in a Job Object.

Timeout measurement and sleeping use an injectable `qubit-clock` timer. This
lets tests drive timeout behavior with a manual monotonic clock. Without a
timeout and without a cancellation handle, the runner waits directly for
process completion instead of polling. Because command execution waits on the
timer synchronously whenever timeout or cancellation is configured, the timer
backend must keep progressing independently of the caller thread. A Tokio timer
must not rely on a current-thread runtime that is driven only by that same
thread.

## Cancellation

`CommandCancellation` is a one-shot handle for applications that already own
their shutdown or terminal-signal policy. Clone it into a runner, then call
`cancel()` from that policy. A request observed before a run starts returns
`CommandError::CancelledBeforeStart` without preparing or spawning the command.
Otherwise, the runner terminates its managed process tree and returns
`CommandError::Cancelled` with retained output. The crate deliberately does not
install a global signal handler.

Configuring cancellation also enables process-tree management for a runner that
uses `without_timeout()`. Cancellation-aware waiting polls the configured timer;
choose a timer backend that keeps progressing independently of the calling
thread.

## Large Output

By default, stdout and stderr are each limited to
`DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM` (currently 1 MiB), and a successful
command whose retained output is truncated returns
`CommandError::OutputTruncated`. For commands that can emit large logs, lower
the memory limit and tee the complete streams to files:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .max_output_bytes(64 * 1024)
    .fail_on_output_truncation(false)
    .tee_stdout_to_file("stdout.log")
    .tee_stderr_to_file("stderr.log")
    .run(Command::new("cargo").arg("test"))?;

if output.stdout_truncated() {
    eprintln!("stdout was truncated in memory; see stdout.log for the full stream");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`bounded_output(max_bytes)` selects a different limit for both streams and
retains the default rejection policy. Use `max_output_bytes(max_bytes)` and
`fail_on_output_truncation(false)` when partial retained output is acceptable.
Only trusted commands whose output is known to remain finite should opt out:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .unbounded_output()
    .run(Command::new("cargo").arg("test"))?;

assert!(!output.stdout_truncated());
# Ok::<(), Box<dyn std::error::Error>>(())
```

An unexpected exit, timeout, or cancellation remains the primary error even
when its retained output is truncated. All four error kinds expose retained
output through `CommandError::output()`.

## Quick Start

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
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

Structured arguments prevent shell parsing, but the target program still
interprets its own options. When a path or other value may start with `-`, use
that program's supported end-of-options marker (commonly `--`) or otherwise
follow its documented argument rules.

## Redacted Diagnostics

Command strings used in runner logs, `CommandError::command()`, and
`Command`'s `Debug` output are redacted with `qubit-redact`.
Sensitive structured argv values such as `--password secret`,
`--access-token=...`, and `OPENAI_API_KEY=...` are masked. Explicit
environment overrides are shown only in redacted form. `Command::shell`
payloads are treated as opaque secrets and are never parsed.

`CommandRunner::new()` snapshots the current process-wide default policy.
Install the application policy before constructing the runner, or inject a
complete immutable policy when that runner needs different rules:

The example below requires a direct `qubit-redact = "0.4"` dependency because
`qubit-command` does not re-export types owned by `qubit-redact`.

```rust
use qubit_command::{Command, CommandRunner};
use qubit_redact::{RedactionPolicy, Sensitivity};

let mut builder = RedactionPolicy::default().to_builder();
builder
    .fields()
    .raise("tenant_option", Sensitivity::Secret)?
    .allow_exact("username")?;
let policy = builder.build()?;
let error = CommandRunner::new()
    .diagnostic_redaction_policy(policy)
    .run(Command::new("__missing__").arg("--tenant-option").arg("secret"))
    .expect_err("sample command should fail");

assert_eq!(
    error.command(),
    r#"["__missing__", "--tenant-option", "<redacted>"]"#,
);
```

Use `Command::sensitive_arg` or `Command::sensitive_arg_os` for positional
values such as customer file paths. The original value is passed unchanged to
the child process, while diagnostics display the configured secret mask.

The runner policy affects runner logs and `CommandError::command()`.
Standalone `Command` `Debug` output has no runner context. Each formatting call
snapshots the process-wide global `RedactionPolicy`; when no policy has been
installed, it uses the standard policy. Use `allow_exact` for a verified
exact-name false positive, or `allow_suffix` only when the broader disclosure is
intentional. Every allow rule should be security-reviewed because matching argv
or environment values can then appear unchanged in diagnostics.

Command lifecycle records are emitted at `debug` level. Calling
`disable_logging(true)` suppresses those records. Cleanup failures that cannot
be returned through `CommandError` may still be logged at `error` level.

`CommandOutput`'s `Debug` output redacts both captured streams and reports only
their byte lengths, truncation flags, status, and elapsed time. Captured
stdout/stderr bytes, their explicit accessors, and tee files remain raw process
output. Use capture limits and caller-side filtering when command output itself
may contain secrets.

Working-directory, stdin-file, and tee-file paths are redacted from `Debug`,
`Display`, and `CommandError` diagnostics. Structured error fields still retain
the raw paths for callers that need to handle the failure programmatically.

## Output Text

`stdout()` and `stderr()` return raw bytes exactly as retained. Use
`stdout_text()` and `stderr_text()` when the command output must be valid UTF-8.
Use `stdout_lossy_text()` and `stderr_lossy_text()` to replace invalid UTF-8
bytes with `�`.

If the captured stdout or stderr contains invalid UTF-8, `stdout_text()` /
`stderr_text()` return `Err(str::Utf8Error)` from `str::from_utf8`. The bytes are
still stored on the returned `CommandOutput`; use `stdout()` / `stderr()` to read
the retained raw output and decode or handle it yourself. When a capture limit
truncates a stream in the middle of a multi-byte sequence, strict decoding can
fail even if the complete process output was valid UTF-8.

When the remaining metadata is no longer needed, `into_stdout()` and
`into_stderr()` move retained bytes out without copying. Likewise,
`CommandError::into_output()` moves retained output out of an error.

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::shell("printf '\\377'"))?;

assert_eq!(output.stdout_lossy_text(), "\u{fffd}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

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
