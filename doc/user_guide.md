# Qubit Command User Guide

[中文用户手册](user_guide.zh_CN.md) · [README](../README.md) · [API documentation](https://docs.rs/qubit-command)

This guide describes `qubit-command` 0.6.0. It is for Rust applications that run external programs and need explicit policies for process lifetime, output size, cancellation, and diagnostics.

## What This Crate Solves

An external command has four useful boundaries:

| Type | Responsibility |
| --- | --- |
| `Command` | Describes which program receives which arguments, environment, working directory, and stdin. |
| `CommandRunner` | Defines how the process is started, waited for, cancelled, logged, and bounded. |
| `CommandOutput` | Carries the observed exit status, retained output, completion flags, and elapsed time. |
| `CommandError` | Explains why preparation, execution, collection, or the success policy did not complete normally. |

`CommandCancellation` connects the runner to a shutdown policy that the application already owns. The crate deliberately does not install a global signal handler.

## Scenario: Run a Repository Check

The success criterion is simple: run `git status --short`, return its stdout as UTF-8, and preserve a typed error if the executable cannot be started or exits unexpectedly.

### Install

Add the crate to the application using Rust 1.94 or newer:

```toml
[dependencies]
qubit-command = "0.6"
```

### Build and Run the Command

Use structured arguments for ordinary commands. This keeps argument boundaries explicit and does not invoke a shell:

```rust
use qubit_command::{Command, CommandRunner};

fn repository_status() -> Result<String, Box<dyn std::error::Error>> {
    let output = CommandRunner::new(std::time::Duration::from_secs(10))
        .run(Command::new("git").args(&["status", "--short"]))?;

    Ok(output.stdout_text()?.to_owned())
}
```

`CommandRunner::new(std::time::Duration::from_secs(10))` uses a ten-second timeout, treats exit code `0` as successful, and retains up to 1 MiB per output stream in memory. `run` waits synchronously until the command succeeds or returns a `CommandError`.

### Decide What Output Means

`stdout()` and `stderr()` return the retained raw bytes. Use `stdout_text()` or `stderr_text()` only when strict UTF-8 is required; use `stdout_lossy_text()` or `stderr_lossy_text()` when invalid bytes should be replaced with `�`.

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::new("printf").arg("hello"))?;

assert_eq!(output.stdout_text()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

If strict decoding fails, the bytes remain available through `stdout()` or `stderr()`. A capture limit can also cut through a multi-byte sequence, so truncation and UTF-8 validity are separate decisions.

## Core Workflow

### Prefer Structured Commands

```rust
let command = Command::new("git")
    .args(&["status", "--short"])
    .working_directory("/workspace/project")
    .env("LC_ALL", "C");
```

Arguments are passed to the target program without shell quoting or expansion. The target program still interprets its own options. If a value may begin with `-`, follow that program's documented argument rules, commonly by placing it after `--`.

For non-UTF-8 program or argument values, use `new_os`, `arg_os`, `args_os`, `env_os`, or `sensitive_arg_os`.

### Use a Shell Only Intentionally

```rust
let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::shell("printf hello | tr a-z A-Z"))?;
assert_eq!(output.stdout_text()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

On Unix-like platforms `Command::shell` runs `sh -c`; on Windows it runs `cmd /C`. The shell payload is treated as an opaque diagnostic secret. Shell expansion, redirection, and pipelines are the caller's responsibility, including input validation.

### Configure Input and Environment

`Command` can inherit stdin, use null stdin, provide bytes, or read from a file. It can inherit the environment, add or override variables, remove variables, or clear the inherited environment before applying explicit values:

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10)).run(
    Command::new("cat")
        .stdin_bytes("input\n")
        .env("LANG", "C"),
)?;

assert_eq!(output.stdout_text()?, "input\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`stdin_file` accepts ordinary files only. Directories, FIFOs, devices, sockets, and other special files are rejected before the child is spawned.

### Define Successful Exit Codes

Exit code `0` is successful by default. If a tool documents another successful status, configure it explicitly:

```rust
let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .success_exit_codes(&[0, 2])
    .run(Command::new("tool"))?;
# let _ = output;
```

An exit status outside the configured list returns `CommandError::UnexpectedExit` and retains the captured output.

## Timeouts and Cancellation

### Timeout

Each `CommandRunner` instance is constructed with an explicit timeout value.
The examples below use ten seconds.
The timeout starts after the child has been spawned, so preparation and
spawning time are outside that duration.

```rust
use std::time::Duration;
use qubit_command::{Command, CommandRunner, CommandRunOptions};

let result = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::new("long-running-tool"));
```

When the deadline is reached, the runner attempts to terminate the managed process tree, collects available output, joins its I/O helpers, and returns `CommandError::TimedOut`. A timeout error can contain partial output.

Use `without_timeout()` only when an unlimited wait is deliberate. If cancellation is configured, the runner still polls its timer and manages the process tree even without a timeout.

### Application-Owned Cancellation

Create one cancellation handle, clone it into the runner, and call `cancel()` from the application's existing shutdown policy:

```rust
use qubit_command::{Command, CommandCancellation, CommandRunOptions, CommandRunner};

let cancellation = CommandCancellation::new();
let runner = CommandRunner::new(std::time::Duration::from_secs(10));
let result = runner.run_with(
    Command::new("long-running-tool"),
    CommandRunOptions::new().cancellation(cancellation.clone()),
);

// Call this from the application's shutdown or terminal-signal policy:
cancellation.cancel();

let result = runner.run_with(
    Command::new("long-running-tool"),
    CommandRunOptions::new().cancellation(cancellation.clone()),
);
```

If cancellation is observed before preparation starts, the result is `CommandError::CancelledBeforeStart`. Otherwise the managed process tree is terminated and the result is `CommandError::Cancelled`, with retained output when available. The handle is one-shot; calling `cancel()` more than once has no additional effect.

For timeout- or cancellation-aware waiting, the configured timer must continue progressing while `run()` blocks synchronously. A Tokio timer must not depend on a current-thread runtime driven only by that same blocked thread.

## Bounded and Large Output

Each stream is limited to `DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM`, currently 1 MiB, by default. A successful command whose retained output is truncated returns `CommandError::OutputTruncated` unless that policy is disabled.

For large logs, keep memory bounded and tee each stream to a file:

```rust
use qubit_command::{Command, CommandRunOptions, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .max_output_bytes(64 * 1024)
    .fail_on_output_truncation(false)
    .run_with(
        Command::new("cargo").arg("test"),
        CommandRunOptions::new()
            .tee_stdout_to_file("stdout.log")
            .tee_stderr_to_file("stderr.log"),
    )?;

if output.stdout_truncated() {
    eprintln!("stdout was truncated in memory; see stdout.log for the full stream");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`bounded_output(max_bytes)` changes both stream limits while retaining the default failure policy. `max_stdout_bytes` and `max_stderr_bytes` configure streams independently. `unbounded_output()` removes both limits and should only be used for trusted commands whose output is known to be finite.

Timeout, cancellation, unexpected exit, and output-truncation errors can expose a `CommandOutput` through `CommandError::output()` or move it with `CommandError::into_output()`. For timeout and cancellation, inspect `stdout_complete()` and `stderr_complete()` before treating retained bytes as complete streams.

Tee paths must be ordinary files. The runner checks stdin/tee and stdout-tee/stderr-tee conflicts before truncating any tee file.
Tee files are truncated and replaced for each run; they are never appended. Cloning `CommandRunOptions` copies the configured paths, so concurrent runs must use distinct paths when their logs must remain separate.

## Diagnostics and Redaction

Runner logs, `CommandError::command()`, and `Command` debug output redact sensitive structured arguments, explicit environment overrides, shell payloads, and configured paths. Use `sensitive_arg` or `sensitive_arg_os` for positional values such as customer file paths:

```rust
let command = Command::new("uploader")
    .arg("--file")
    .sensitive_arg("customer-report.csv");
```

The original value is passed to the child unchanged, while diagnostic rendering uses a mask. `CommandRunner::new(std::time::Duration::from_secs(10))` snapshots the process-wide default redaction policy. A runner can instead receive a complete immutable policy through `diagnostic_redaction_policy`; the policy type belongs to `qubit-redact`, so applications must depend on that crate directly when constructing one.

`allow_exact` and `allow_suffix` rules can expose values in diagnostics. Use them only after reviewing the exact disclosure boundary. `CommandOutput` debug output redacts captured streams and reports metadata; the explicit byte accessors and tee files remain raw process output.

Lifecycle records are emitted at `debug` level. `disable_logging(true)` suppresses those records, while cleanup failures that cannot be returned through `CommandError` may still be logged at `error` level.

## Errors and Diagnostics

`CommandError` is non-exhaustive, so downstream matches must keep a wildcard arm. The important categories are:

| Category | Examples | Next diagnostic step |
| --- | --- | --- |
| Preparation | `OpenInputFailed`, `NonRegularInputFile`, `InputOutputConflict`, `OutputFilesConflict` | Verify paths, file kinds, and that input/output paths are distinct. |
| Process control | `SpawnFailed`, `WaitFailed`, `KillFailed`, `CancelFailed` | Check the executable, permissions, platform process-control support, and the source I/O error. |
| Stream I/O | `ReadOutputFailed`, `WriteInputFailed`, `OpenOutputFailed`, `WriteOutputFailed` | Check the named stream, file access, and retained output attached to the error when present. |
| Time | `TimeFailed` | Check that the timer and clock share a valid monotonic domain and can progress while the caller is blocked. |
| Policy result | `UnexpectedExit`, `OutputTruncated`, `TimedOut`, `Cancelled`, `CancelledBeforeStart` | Inspect status, configured policy, retained output, and stream-completion flags. |
```

For a policy error with output:

```rust
use qubit_command::{Command, CommandError, CommandRunner};

match CommandRunner::new(std::time::Duration::from_secs(10)).run(Command::new("tool")) {
    Ok(output) => println!("{}", output.stdout_lossy_text()),
    Err(error) => {
        eprintln!("{}", error);
        if let Some(output) = error.output() {
            eprintln!("stdout complete: {}", output.stdout_complete());
            eprintln!("stderr complete: {}", output.stderr_complete());
        }
        if matches!(error, CommandError::TimedOut { .. }) {
            eprintln!("the process exceeded its timeout");
        }
    }
}
```

## Troubleshooting

### The command cannot be found

`SpawnFailed` means the operating system could not start the requested program. Confirm the program name/path, the process environment, and the working directory. Use `Command::new_os` if the executable path is not valid UTF-8.

### The command exits with an unexpected status

Read `CommandError::UnexpectedExit`, inspect `exit_code`, `expected`, stdout, and stderr, then decide whether the status is genuinely successful for the application. Do not silently treat every non-zero exit as success.

### The command times out or is cancelled

Use the retained output for diagnostics, but check both completion flags. Descendants or blocked I/O can mean that a retained stream is not complete even after cleanup returns.

### Output is truncated

Keep the bounded setting and use tee files for complete logs, or increase the stream limit. Disable failure on truncation only when the application explicitly accepts partial in-memory output. Avoid `unbounded_output()` for untrusted or open-ended commands.

### Text decoding fails

Use raw `stdout()`/`stderr()` for binary output or strict decoding with your own codec. Use the lossy accessor when replacement characters are acceptable. Truncation can split a valid UTF-8 sequence.

### A file configuration is rejected

Ensure stdin and tee paths identify ordinary files, not directories or special files, and ensure no two configured paths identify the same file. These checks happen before the child starts and before tee files are truncated.

### A diagnostic contains too much or too little information

Use `sensitive_arg` for caller-known secrets and review redaction allow rules carefully. Remember that explicit output accessors and tee files contain raw process output; diagnostic redaction does not filter that data.

## Limitations and Best Practices

- Prefer `Command::new` with explicit arguments. Use `Command::shell` only when shell behavior is part of the requirement.
- Keep the default timeout and bounded capture unless the command's behavior justifies a different policy.
- Treat timeout and cancellation output as potentially incomplete.
- Use tee files instead of unbounded memory capture for large logs.
- Keep stdin and tee paths as separate ordinary files.
- Configure cancellation from the application's existing shutdown policy; this crate does not install global signal handlers.
- Choose a timer backend that progresses independently of the synchronous caller when timeout or cancellation is enabled.
- Treat captured output and tee files as untrusted process data even though diagnostics are redacted.

## Related Resources

- [README](../README.md)
- [中文 README](../README.zh_CN.md)
- [中文用户手册](user_guide.zh_CN.md)
- [API documentation](https://docs.rs/qubit-command)
- [Command-runner I/O lifecycle design](command-runner-io-lifecycle-design.md)
