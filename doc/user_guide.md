# Qubit Command User Guide

[中文用户手册](user_guide.zh_CN.md) · [README](../README.md) · [API documentation](https://docs.rs/qubit-command)

This guide describes `qubit-command` 0.12. It is for Rust applications that run external programs and need explicit policies for process lifetime, output size, cancellation, resource ownership, and diagnostics.

## Managed Execution for Service Work

Use the managed API when a service needs bounded command admission, process-tree lifetime management, and an explicit resource-settlement result. The example below creates one `OperationManager`, attaches one `CommandExecutor`, and collects a repository status command:

```toml
[dependencies]
qubit-command = "0.12"
qubit-operation = "^0.3"
```

<!-- doc-example:managed-service-quick-start:compile -->
```rust
use qubit_command::{Command, CommandExecutionConfig, CommandExecutor, CommandRunOptions, CommandRunner, ManagedCommandRunnerApi};
use qubit_operation::{ManagerOptions, OperationManager};

let manager = OperationManager::new(ManagerOptions::default())?;
let executor = CommandExecutor::new(manager, CommandExecutionConfig::default())?;
let output = CommandRunner::without_timeout()
    .run_managed(&executor, Command::new("git").args(&["status", "--short"]), CommandRunOptions::new())?;
println!("{}", output.stdout_lossy_text());
# Ok::<(), Box<dyn std::error::Error>>(())
```

The default executor allows 4 active reservations, 64 queued reservations, and 64 capacity waiters. Its legacy-named `max_queued_input_bytes` defaults to 16 MiB and limits the estimated file bytes held by **all active and queued reservations**, not just queued work. Ordinary `CommandRunOptions` submissions, including `stdin_bytes`, estimate to zero. `CommandResourceRunOptions` estimates the sum of the metadata lengths of each present stdin, stdout, and stderr file. The estimate uses full file lengths (including existing output contents before truncation), does not move file cursors, and contributes zero for metadata lookup failures; integer conversion and summation saturate. It is an admission estimate, not a limit on process memory, buffered stdin, or actual output. Capacity waiters have not reserved capacity yet and are not included, although their callers may still hold inputs.

A zero `max_queued_input_bytes` therefore admits zero-estimate submissions, including ordinary stdin byte buffers, but rejects resource files with nonzero estimated length. A zero `max_queued` disables queueing while still allowing immediate admission into an available active slot; a zero `max_waiters` disables registering capacity waiters. `max_active` must be nonzero. Reservation bytes remain counted while the reservation record is retained: dropping the owner alone does not release them while a lease remains, and receiving an operation outcome does not itself prove that the reservation has been removed. These are executor-local limits; configure the shared `OperationManager` separately for application-wide operation limits.

`max_diagnostics` remains in the configuration for compatibility but has no effect: the executor does not read it, and zero behaves like every other value. A stopped operation has a single diagnostic slot, and duplicate stop diagnostics are not published. This does not imply a bound on total diagnostic memory across operations.

Managed execution defaults to `ProcessTreePolicy::Managed`, independently of timeout and cancellation options. Select `ProcessTreePolicy::DirectChild` only when the application intentionally manages the direct child alone. This policy applies to managed execution; the older `CommandRunner::run` and `run_with` path remains available with its existing lifecycle contract.

### Observe outcome and settlement separately

Use `CommandExecutor::submit` or `submit_in` when the caller must distinguish the typed operation result from resource cleanup. `wait_outcome` consumes the one-shot outcome; `wait_settlement` observes the later stable cleanup report. A successful outcome does not itself prove that every helper or owned file has been released. The [collect example](../examples/managed_collect.rs) starts in a parent `OperationScope`, moves its handle to the async view, awaits the outcome, then checks settlement separately. Never block a finite synchronous worker while it waits for dependent work that needs the same worker pool.

The synchronous and asynchronous handles share one outcome receiver: convert with `into_async` or `into_sync` before taking the result. For resource-bearing operations, keep the handle or a read-only observer until settlement is known. `SettlementStatus::RecoveryRequired` means recovery evidence exists; it does not guarantee that an application recovery handle is available. First inspect the manager report's `status`, `effects`, `effect_overflow`, recovery counts, and truncation flags, then inspect the same evidence in `external_settlement` when present. After reviewing both reports, inspect the manager report's `recoveries` for handles available to this managed command flow, and act only on handles actually present there. The adapter may already have run recovery logic: its `external_settlement.recovery_count` can be greater than zero while its `recoveries` array and the manager report's `recoveries` are empty, leaving no handle for the application to retry. Use release and effect evidence from both reports to decide whether staged files can be removed. Do not delete them merely because a recovery-handle array is empty.

When a managed command stops, keep its concrete `CommandOperation` (or `AsyncCommandOperation`) through both observations. A `Stopped` outcome contains a stop report, not `CommandOutput`; do not invent an exit status. Wait for settlement before calling `take_stop_diagnostic`, because backend cleanup publishes the optional diagnostic during settlement. The method takes an `Option<CommandError>` once. `None` does not undo or disprove the observed stop. Read the diagnostic's stop reason, `cleanup_failures()`, and any `captured_output()` metadata. Captured bytes may be partial: check each stream's truncation and EOF completeness flags. EOF only describes collection, not command success. Raw bytes may contain secrets, so retain or disclose them according to application policy instead of logging them by default. A read-only operation observer cannot take this command-specific diagnostic; `into_async` and `into_sync` preserve access, while `into_inner` gives it up. Keep the settlement report and follow the application's recovery process whenever it reports `RecoveryRequired`.

This sync helper shows the observation order and returns the outcome, settlement, and optional diagnostic to its caller. The caller remains responsible for recovery and for choosing how to handle captured bytes:

<!-- doc-example:managed-stop-diagnostic:compile -->
```rust
use std::sync::Arc;
use qubit_command::{CommandError, CommandOperation};
use qubit_operation::{OperationOutcome, SettlementReport};

fn observe_stopped_command(
    mut handle: CommandOperation,
) -> Result<(
    OperationOutcome<qubit_command::CommandOutput, CommandError>,
    Arc<SettlementReport>,
    Option<CommandError>,
), Box<dyn std::error::Error>> {
    let outcome = handle.wait_outcome()?;
    let settlement = handle.wait_settlement()?;
    let diagnostic = if matches!(&outcome, OperationOutcome::Stopped(_)) {
        handle.take_stop_diagnostic()
    } else {
        None
    };

    if let Some(error) = diagnostic.as_ref() {
        if let Some(captured) = error.captured_output() {
            let _metadata = (
                captured.stdout().len(),
                captured.stdout_truncated(),
                captured.stdout_complete(),
                captured.stderr().len(),
                captured.stderr_truncated(),
                captured.stderr_complete(),
            );
        }
        let _cleanup_failures = error.cleanup_failures();
    }
    // Return RecoveryRequired and its recovery handles to the application.
    Ok((outcome, settlement, diagnostic))
}
```

### Stream a command with a session

Enable the crate's `async` feature to use `start_session`. A session exposes stdin, stdout, and stderr as independent byte endpoints. Read stdout and stderr concurrently so a full pipe cannot block the other stream; close stdin explicitly to send EOF. After collecting the operation outcome, drop the endpoints and then wait for settlement. The [session example](../examples/managed_session.rs) demonstrates this order using the runtime-independent `qubit-io` traits.

If the caller drops one output endpoint early, the session switches that stream to discard-and-drain behavior and the result may be incomplete. `flush` only means bytes were accepted by the endpoint's pipe writer; it does not mean the child consumed them. `BrokenPipe` is an I/O fact to handle independently from the operation's primary outcome.

Each session output queue is bounded to 64 KiB. While the shared stdout/stderr session budget has not been exceeded, a full queue applies backpressure and preserves the bytes; read both streams concurrently so the child cannot block behind an unread pipe. Reaching the configured budget exactly is allowed. The first observed byte beyond it triggers the output-limit failure and later bytes are drained as incomplete output. Dropping an endpoint explicitly switches that stream to discard-and-drain behavior.

### Pass owned files explicitly

`CommandResourceRunOptions` transfers `File` handles to a managed run. An stdin file starts at its current cursor; the library does not seek it implicitly. Each stdout/stderr sink must choose `FileSinkPolicy::Preserve` or `FileSinkPolicy::Truncate`. Truncation is an observable side effect and occurs only after command and resource capacity have been reserved. Identity conflicts are checked using opened handles, so equal paths are neither a lease nor the authority token. See the [owned-file example](../examples/managed_file.rs).

Wait for settlement before deleting an external temporary directory that contains a file still used by the operation. Once settlement confirms release, the application remains responsible for its own directory cleanup.

### Submission paths and typed wait failures

Managed stdin and tee relative paths use the parent process directory captured when a waiting submission first executes; admission and manager-Busy retries keep that same base. Child working directory selection is separate, and a command override takes precedence over the runner default. If the application needs a new cwd as the relative-path base, pass absolute paths or make a new submission. On Windows, drive-relative paths keep platform semantics. The request snapshot compatibility `environment` field is empty; the child still inherits its environment at process start and applies explicit clear, remove, and override settings.

Resource submission prechecks incompatible configuration before registration, then checks actual opened file identities at final commit before any truncation or spawn. Owned stdin cannot be combined with command file or byte stdin; choose one source. Session stdin cannot be combined with file stdin, while its supported byte prefix remains available. Tee paths can be created before a later commit error, and a failure after one sink is truncated can leave that sink changed. Resource-backed path stdin open and non-regular-file failures use `FilePreparationFailed` at `Validation` with direction `Stdin`. The original I/O source is retained when present. Tee validation failures use `Validation` with direction `Stdout` or `Stderr`. An actual `set_len` failure uses `Truncation` and that sink direction. The source remains available through `CommandErrorReason::FilePreparationFailed`. For manager classification retained from resource registration, call `CommandError::resource_registration_error_kind()`. Callers that matched `OpenInputFailed` or `OpenOutputFailed` for resource-backed preparation should switch to the public `FilePreparationFailed` classification and inspect its stage, direction, and source. Ordinary path-based legacy errors keep their existing classification.

A stop accepted before process start is returned by an operation handle as `OperationOutcome::Stopped`; after settlement, the concrete command handle can provide its optional stop diagnostic through `take_stop_diagnostic()`. Collecting `run_managed*` entry points continue to expose their existing `CommandError` result mapping. A stop requested on one operation affects that operation only. To stop several runs, the application can pass clones of one shared `CommandCancellation` token to them and call `cancel()` on that token. The first stop reason accepted for a run remains its primary reason; later stop signals do not replace it. In normal operation, the reaper accepts cleanup owners and releases them after cleanup completes. Reaper admission is bounded: a full queue backs off and retries. If the service is disconnected or cannot start, execution tries once to obtain a healthy pool, then cleans up on the current bounded execution worker if it remains unavailable. Recovery owners use the existing bounded recovery worker for the same fallback, which may wait for real OS cleanup. The worker publishes release after actual cleanup completes. The lease remains held until upstream settlement completes. Continue observing settlement or shutdown to confirm when resources are released. A scoped capacity wait returns `AdmissionError::Stopped` after observing scope stop and `CapacityFailure::DeadlineExpired` after deadline expiry. Once a waiter is registered, executor shutdown ends that wait with `Capacity(Closed)`; stop, deadline, and closure are determined at the wait decision point, with deadline taking precedence over stop, and stop over closure. `CommandSubmitFailureReason::Wait(CommandSubmitWaitFailure)` distinguishes `WaiterLimit` from `Timer(RetryTimerFailure)`. Timer failures include startup and queue-full failures before acceptance, as well as disconnection; `Disconnected` can also occur after successful queueing if the timer worker exits. In every case the submit failure retains the original runner, command, and options, including owned resource inputs. `max_waiters` covers both local capacity waits and manager `Busy` waits. Exhaustive matches on the submit failure reason must handle this variant.

### Shut down without guessing

`CommandExecutor::request_shutdown` stops new executor admission and returns a durable shutdown observer. Use `wait_for(Duration::ZERO)` to inspect an immediate snapshot. `ShutdownWait::Pending` is not failure: retain that same handle and continue with `wait` or another bounded `wait_for`. Do not infer release from elapsed time or a fixed sleep. The [shutdown example](../examples/managed_shutdown.rs) keeps an stdin-gated session active, confirms the child has consumed input by reading its response, observes `Pending`, then closes stdin and releases endpoints before continuing to observe shutdown. It also matches `SettlementStatus::RecoveryRequired` and reports retained recovery handles.

Dropping the final `CommandExecutor` clone closes its local scheduler to new submissions and lets already accepted work drain in the background. Drop is non-blocking and does not prove that operations have settled or recovery has finished. Keep an operation handle or observer when settlement matters; use explicit shutdown when the caller needs a completion handle.

On Windows, the existing helper-cancellation limitation still applies: successful cancellation does not prove a helper has stopped, and the runner does not promise a hard wall-clock return deadline. See the [Windows cancellation status](windows-cancellation-status.md).

### Executable examples

Run the examples from the repository with `cargo run --features async --example managed_collect` (replace the example name with `managed_session`, `managed_shutdown`, or `managed_file`). They use only the public API; the session endpoint example requires the `async` feature. See the [0.12 migration guide](migration-0.12.md) before moving existing call sites.

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
qubit-command = "0.12"
```

### Build and Run the Command

Use structured arguments for ordinary commands. This keeps argument boundaries explicit and does not invoke a shell:

<!-- doc-example:basic-run:compile -->
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

Requires a Unix-like environment with the `printf` utility:

<!-- doc-example:unix-output:unix -->
```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::new("printf").arg("hello"))?;

assert_eq!(output.stdout_text()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

On Windows, use Windows PowerShell with structured arguments instead:

<!-- doc-example:windows-output:windows -->
```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::new("powershell").args(&[
        "-NoProfile", "-Command", "[Console]::Write('hello')",
    ]))?;

assert_eq!(output.stdout_text()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

If strict decoding fails, the bytes remain available through `stdout()` or `stderr()`. A capture limit can also cut through a multi-byte sequence, so truncation and UTF-8 validity are separate decisions.

## Core Workflow

### Prefer Structured Commands

<!-- doc-example:structured-command:compile -->
```rust
use qubit_command::Command;

let command = Command::new("git")
    .args(&["status", "--short"])
    .working_directory("/workspace/project")
    .env("LC_ALL", "C");
# let _ = command;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Arguments are passed to the target program without shell quoting or expansion. The target program still interprets its own options. If a value may begin with `-`, follow that program's documented argument rules, commonly by placing it after `--`.

For non-UTF-8 program or argument values, use `new_os`, `arg_os`, `args_os`, `env_os`, or `sensitive_arg_os`.

### Use a Shell Only Intentionally

The script below uses Unix shell syntax and requires a Unix-like environment with `sh`, `printf`, and `tr`:

<!-- doc-example:unix-shell:unix -->
```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::shell("printf hello | tr a-z A-Z"))?;
assert_eq!(output.stdout_text()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

On Windows, `Command::shell` uses `cmd /C`; provide a `cmd` script, not a PowerShell script. This example prints `hello` (the trailing line ending is ignored):

<!-- doc-example:windows-shell:windows -->
```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::shell("echo hello"))?;
assert_eq!(output.stdout_text()?.trim(), "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

On Unix-like platforms `Command::shell` runs `sh -c`; on Windows it runs `cmd /C`. Its generated script argument is treated as an opaque diagnostic secret. Whole-script automatic redaction is limited to this generated form and recognized shell argument forms; manually constructed scripts such as `bash -c` and `powershell -Command` are not guaranteed to be hidden as a whole. Mark a sensitive script argument with `sensitive_arg` or `sensitive_arg_os`. Shell expansion, redirection, and pipelines are the caller's responsibility, including input validation.

### Configure Input and Environment

`Command` can inherit stdin, use null stdin, provide bytes, or read from a file. It can inherit the environment, add or override variables, remove variables, or clear the inherited environment before applying explicit values:

Requires a Unix-like environment with the `cat` utility:

<!-- doc-example:unix-stdin:unix -->
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

On Windows, this structured-argument version requires Windows PowerShell. It echoes the sample text through text input/output APIs; it is not a binary byte-preserving copy example.

<!-- doc-example:windows-stdin:windows -->
```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10)).run(
    Command::new("powershell").args(&[
        "-NoProfile", "-Command", "[Console]::Out.Write([Console]::In.ReadToEnd())",
    ]).stdin_bytes("input\n"),
)?;

assert_eq!(output.stdout_text()?, "input\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`stdin_file` accepts ordinary files only. Directories, FIFOs, devices, sockets, and other special files are rejected before the child is spawned.

Relative paths passed to `stdin_file`, `tee_stdout_to_file`, or
`tee_stderr_to_file` are resolved from the parent process's current working
directory. `Command::working_directory` and `CommandRunner::working_directory`
change only the child process directory. To address a file under the child
directory, construct an absolute path before configuring the file:

<!-- doc-example:path-configuration:compile -->
```rust
use qubit_command::Command;

let child_dir = std::env::current_dir()?.join("workspace").join("project");
let input_path = child_dir.join("input.txt");
let command = Command::new("tool")
    .working_directory(&child_dir)
    .stdin_file(input_path);
# let _ = command;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Define Successful Exit Codes

Exit code `0` is successful by default. If a tool documents another successful status, configure it explicitly:

<!-- doc-example:success-exit-codes:compile -->
```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .success_exit_codes(&[0, 2])
    .run(Command::new("tool"))?;
# let _ = output;
```

An exit status outside the configured list returns a `CommandError` whose
`kind()` is `CommandErrorKind::UnexpectedExit` and retains the captured output.

## Timeouts and Cancellation

### Timeout

`CommandRunner::new(timeout)` sets an explicit timeout duration;
`CommandRunner::without_timeout()` disables the timeout. The examples below
use ten seconds.
The timeout starts after the child has been spawned, so preparation and
spawning time are outside that duration.

<!-- doc-example:timeout-configuration:compile -->
```rust
use std::time::Duration;
use qubit_command::{Command, CommandRunner};

let result = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::new("long-running-tool"));
```

`CommandRunner::new(timeout)` uses the timeout supplied by the caller; `CommandRunner::without_timeout()` explicitly disables the timeout.

When the deadline is reached, the runner attempts to terminate the managed process tree, collects available output, joins its I/O helpers, and returns a `CommandError` whose `kind()` is `CommandErrorKind::TimedOut`. A timeout error can contain partial output.

Use `without_timeout()` only when an unlimited wait is deliberate. If cancellation or a total output budget is configured, the runner still polls its timer and manages the process tree even without a timeout.

### Application-Owned Cancellation

Create one cancellation handle, clone it into the runner, and call `cancel()` from the application's existing shutdown policy:

<!-- doc-example:application-cancellation:compile -->
```rust
use qubit_command::{Command, CommandCancellation, CommandRunOptions, CommandRunner};

let cancellation = CommandCancellation::new();
let runner = CommandRunner::new(std::time::Duration::from_secs(10));
let worker_runner = runner.clone();
let worker_cancellation = cancellation.clone();
let worker = std::thread::spawn(move || {
    worker_runner.run_with(
        Command::new("long-running-tool"),
        CommandRunOptions::new().cancellation(worker_cancellation),
    )
});

// Call this from the application's shutdown or terminal-signal policy while
// the worker is running:
cancellation.cancel();
let result = worker.join().expect("command worker should not panic");
```

If cancellation is observed before the final startup check, the result is a
`CommandError` whose `kind()` is `CommandErrorKind::CancelledBeforeStart`; no tee file is created
or truncated and no child is spawned. The final check is the startup
linearization point. Cancellation observed after that point is an in-flight
request: the managed process tree is terminated and `kind()` is `Cancelled`,
with retained output when available. The handle is one-shot; calling `cancel()`
more than once has no additional effect.

Timeout, cancellation, and total-output-budget monitoring use timer-based polling. This includes a total output budget configured by itself. The configured timer must continue progressing independently while `run()` blocks synchronously. A Tokio timer must not depend on a current-thread runtime driven only by that same blocked thread.

On Windows, a successful `CancelSynchronousIo` means the cancellation request
succeeded; it does not prove that the helper has stopped. The runner then joins
the helper, and that join is not limited by the 100 ms deadline. The runner
establishes one shared 100 ms helper-confirmation deadline before issuing
cancellation requests and consults it only when a request fails; it is not
reset for each helper or a hard deadline for the whole call. If that deadline expires,
the returned error retains a cancellation cleanup failure so the result stays
bounded; the affected helper may remain alive until its pipe closes. The
current investigation status and evidence are summarized in the [Windows
cancellation status](windows-cancellation-status.md); the [RC-002 verification
history](rc-002-verification.md#current-summary) retains the detailed audit trail.

## Bounded and Large Output

Each stream is limited to `DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM`, currently 1 MiB, by default. A successful command whose retained output is truncated returns a `CommandError` whose `kind()` is `CommandErrorKind::OutputTruncated` unless that policy is disabled.

For large logs, keep memory bounded and tee each stream to a file. A per-stream
retention limit alone does not stop draining or tee writes. A total output
budget, timeout, cancellation, or pipe read failure can stop collection. A tee
write failure can leave the tee file incomplete while draining continues; a
tee flush failure can leave it incomplete after the stream was drained:

<!-- doc-example:tee-output:compile -->
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
    eprintln!("stdout was truncated in memory; see stdout.log for collected output");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`bounded_output(max_bytes)` sets both per-stream retention caps and enables failure on truncation, overriding an earlier failure-policy setting. `max_stdout_bytes` and `max_stderr_bytes` configure streams independently. These limits control retained memory; they do not stop a command from emitting more bytes. Set `max_total_output_bytes(limit)` to impose an opt-in hard limit shared by stdout and stderr. Exceeding it stops the command according to the selected `ProcessTreePolicy`; `DirectChild` stops only the direct child. It returns `CommandErrorKind::OutputLimitExceeded`; inspect `CommandErrorReason::OutputLimitExceeded { limit, observed }` for the configured limit and observed sentinel byte. The partial bytes are available through `CommandError::captured_output()`, while `CommandError::output()` remains `None` because no natural process status is fabricated. `without_total_output_limit()` removes the hard limit. `unbounded_output()` removes both per-stream retention caps and disables failure on truncation, but does not remove a configured total output budget. The total budget limits observed bytes; it is not a hard wall-clock deadline and does not bound preparation, spawning, or arbitrary filesystem operations.

Both `bounded_output(max_bytes)` and `unbounded_output()` override an earlier `fail_on_output_truncation(...)` call, so call order matters. The `max_*_bytes` methods change retention limits only. For example, `unbounded_output().max_output_bytes(1024)` keeps truncation failures disabled; adding `.fail_on_output_truncation(true)` last enables them. Conversely, `fail_on_output_truncation(false).bounded_output(1024)` enables them again. These per-stream settings do not change the combined output budget.

For example, inspect the limit error's classification and partial capture separately from ordinary command output:

<!-- doc-example:total-output-budget:no_run -->
```rust,no_run
use qubit_command::{Command, CommandErrorKind, CommandRunner};

let result = CommandRunner::without_timeout()
    .max_total_output_bytes(1024)
    .run(Command::new("tool"));

if let Err(error) = result
    && error.kind() == CommandErrorKind::OutputLimitExceeded
{
    assert!(error.output().is_none());
    if let Some(captured) = error.captured_output() {
        eprintln!("retained {} stdout bytes", captured.stdout().len());
    }
}
```

Timeout, cancellation, unexpected exit, and output-truncation errors can expose a `CommandOutput` through `CommandError::output()` or move it with `CommandError::into_output()`. For timeout and cancellation, inspect `stdout_complete()` and `stderr_complete()` before treating retained bytes as complete streams.

Use `CommandError::into_captured_output()` when you need to move captured bytes into an independent value, including bytes retained by an output-budget error whose `output()` is `None`. The method prefers a retained `CommandOutput` when both output carriers exist and preserves the stream completeness and truncation markers. It moves the buffers without cloning them and consumes the error and its diagnostics, so inspect any needed reason or cleanup details first. Captured bytes are raw process output and may contain sensitive data; handle them accordingly.

Tee paths must be ordinary files. The runner checks stdin/tee and stdout-tee/stderr-tee conflicts before truncating any tee file. These checks reduce destructive conflicts but do not make file preparation transactional:

1. Validation and file-identity checks are intended to prevent conflicting files from being truncated; they do not guarantee that every failure has no file side effect.
2. Commit opens stdout before stderr. If opening stderr fails, a previously nonexistent stdout path may already exist as an empty file.
3. After both handles pass identity checks, commit truncates stdout and then stderr. If stderr truncation fails, stdout may already be empty while stderr retains its previous bytes; the child has not started.
4. A spawn failure after file commit does not restore the previous tee contents.
5. To retain prior logs, use a distinct path for each run and let the application archive or replace files after success. The library does not promise atomic replacement.

Tee files are truncated and replaced for each run; they are never appended. Cloning `CommandRunOptions` copies the configured paths, so concurrent runs must use distinct paths when their logs must remain separate. No child process does not mean no tee file side effects.

## Diagnostics and Redaction

Runner logs, `CommandError::command()`, and `Command` debug output redact explicitly sensitive structured arguments, explicit environment overrides, configured paths, and the whole script generated by `Command::shell`. Automatic whole-script redaction has limited interpreter coverage; general argument heuristics may redact parts of other scripts but do not guarantee hiding the complete script. Use `sensitive_arg` or `sensitive_arg_os` for positional values such as customer file paths and for manually constructed interpreter scripts:

<!-- doc-example:sensitive-argument:compile -->
```rust
use qubit_command::Command;

let command = Command::new("uploader")
    .arg("--file")
    .sensitive_arg("customer-report.csv");

let powershell = Command::new("powershell")
    .arg("-Command")
    .sensitive_arg("Write-Output 'customer-script-marker'");
```

The original value is passed to the child unchanged, while diagnostic rendering uses a mask. `CommandRunner::new(std::time::Duration::from_secs(10))` snapshots the process-wide default redaction policy. A runner can instead receive a complete immutable policy through `diagnostic_redaction_policy`; the policy type belongs to `qubit-redact`, so applications must depend on that crate directly when constructing one.

`allow_exact` and `allow_suffix` rules can expose values in diagnostics. Use them only after reviewing the exact disclosure boundary. `CommandOutput` debug output redacts captured streams and reports metadata; the explicit byte accessors and tee files remain raw process output.

Lifecycle records are emitted at `debug` level. `disable_logging(true)` suppresses those records, while cleanup failures that cannot be returned through `CommandError` may still be logged at `error` level.

## Errors and Diagnostics

`CommandError` is a stable container with private storage. Use `kind()` for
forward-compatible classification and `reason()` only when detailed fields are
needed; do not match its storage layout. The important categories are:

`CommandError` display/debug output, `CommandErrorReason` debug output, and
cleanup-failure debug output use controlled summaries for I/O sources: the
error kind and raw OS code when available. Time-source details are omitted.
These summaries do not format arbitrary source text. The original source
objects remain available through the error APIs and may contain unfiltered
text; explicitly formatting a full error chain is the caller's responsibility.

| Category | Examples | Next diagnostic step |
| --- | --- | --- |
| Preparation | Path-based legacy errors: `OpenInputFailed`, `NonRegularInputFile`. Resource-backed errors: `FilePreparationFailed` | For resource-backed failures, inspect `reason()` for `Validation`/`Stdin`, `Validation`/`Stdout` or `Stderr`, or `Truncation` and the corresponding direction. Inspect its source when present. Also verify paths, file kinds, and input/output identity. |
| Process control | `SpawnFailed`, `WaitFailed` | Check the executable, permissions, platform process-control support, and the source I/O error. |
| Stream I/O | `ReadOutputFailed`, `WriteInputFailed`, `OpenOutputFailed`, `WriteOutputFailed` | Check the named stream, file access, and retained output attached to the error when present. |
| Time | `TimeFailed` | Check that the timer and clock share a valid monotonic domain and can progress while the caller is blocked. |
| Output budget | `OutputLimitExceeded` | Inspect `reason()` for `limit` and `observed` (including the sentinel byte), then read `captured_output()` and check stream completeness and truncation. `output()` is `None`, which does not mean no bytes were retained. See [Bounded and Large Output](#bounded-and-large-output). |
| Policy result | `UnexpectedExit`, `OutputTruncated`, `TimedOut`, `Cancelled`, `CancelledBeforeStart` | Inspect status, configured policy, retained output, and stream-completion flags. |

The runner polls completed stdout/stderr readers while it waits for the child and
for inherited output pipes after child exit. If a read failure is observed before
another terminal event, the runner stops the child; after successful
termination it returns the collector's original `ReadOutputFailed` error with
retained partial output. If termination fails, the process-control error remains
primary and reader errors remain cleanup details. An earlier tee
write failure on the same stream remains primary under the existing collector
ordering. Budget, child-wait, cancellation, timeout, and clock events keep their
existing priority once selected. Managed execution follows its selected
`ProcessTreePolicy`: the default `Managed` policy manages the process tree, and
explicit `DirectChild` stops only the direct child. Legacy `run`/`run_with` use
adaptive 1–10 ms polling and attempt process-tree management when timeout,
cancellation, or a total output budget is configured. Otherwise they stop only
the direct child. A reader failure alone does not enable process-tree
management for legacy calls. A read failure observed before
the deadline can change a previous `TimedOut` result into `ReadOutputFailed`, so
callers that retry by error kind should account for incomplete output. Process
termination and ordinary-file I/O are not hard-deadline operations. When using
partial output, inspect the primary error, cleanup failures, and stream-completeness
flags.
Downstream adapters such as `rs-mime`'s `SystemMimeCommandExecutor` should
propagate `ReadOutputFailed` and must not parse its partial output as business
data.

For a policy error with output:

<!-- doc-example:policy-error:compile -->
```rust
use qubit_command::{Command, CommandErrorKind, CommandRunner};

match CommandRunner::new(std::time::Duration::from_secs(10)).run(Command::new("tool")) {
    Ok(output) => println!("{}", output.stdout_lossy_text()),
    Err(error) => {
        eprintln!("{}", error);
        if let Some(output) = error.output() {
            eprintln!("stdout complete: {}", output.stdout_complete());
            eprintln!("stderr complete: {}", output.stderr_complete());
        }
        if error.kind() == CommandErrorKind::TimedOut {
            eprintln!("the process exceeded its timeout");
        }
        if !error.cleanup_failures().is_empty() {
            eprintln!("{} cleanup failure(s) were retained", error.cleanup_failures().len());
        }
    }
}
```

## Failure and cleanup policy in 0.8

Once a timeout, cancellation, wait error, or clock error is selected, it remains
the primary `CommandError::kind()`. Later termination and I/O failures appear in
`cleanup_failures()`, including `CommandCleanupFailure::Time` for a clock failure
during finalization. Available partial output is moved into the primary error;
status or elapsed time that cannot be established is never fabricated.

Startup failures explicitly clean up all resources before returning. If both
termination requests fail and the child status is unknown, the runner returns
that evidence without entering an unbounded wait. The child may remain alive;
this is not a hard deadline for arbitrary operating-system or filesystem calls.
See the [0.8 migration guide](migration-0.8.md) for the removed error variants.

## Troubleshooting

### The command cannot be found

`SpawnFailed` means the operating system could not start the requested program. Confirm the program name/path, the process environment, and the working directory. Use `Command::new_os` if the executable path is not valid UTF-8.

### The command exits with an unexpected status

Check `error.kind() == CommandErrorKind::UnexpectedExit`, then inspect
`error.exit_code()`, `error.reason()`, stdout, and stderr before deciding
whether the status is genuinely successful for the application. Do not
silently treat every non-zero exit as success.

### The command times out or is cancelled

Use the retained output for diagnostics, but check both completion flags. Descendants or blocked I/O can mean that a retained stream is not complete even after cleanup returns.

### Output is truncated

Keep the bounded setting and use tee files to retain output beyond the in-memory limit. A per-stream retention limit alone does not stop draining or tee writes, but a total output budget, timeout, cancellation, or pipe read failure can stop collection. A tee write failure can leave the tee file incomplete while draining continues; a tee flush failure can leave it incomplete after the stream was drained. Disable failure on truncation only when the application explicitly accepts partial in-memory output. Avoid `unbounded_output()` for untrusted or open-ended commands.

### Text decoding fails

Use raw `stdout()`/`stderr()` for binary output or strict decoding with your own codec. Use the lossy accessor when replacement characters are acceptable. Truncation can split a valid UTF-8 sequence.

### A file configuration is rejected

Ensure stdin and tee paths identify ordinary files, not directories or special files, and ensure no two configured paths identify the same file. These checks happen before the child starts and before tee files are truncated.

### A diagnostic contains too much or too little information

Use `sensitive_arg` for caller-known secrets and review redaction allow rules carefully. Remember that explicit output accessors and tee files contain raw process output; diagnostic redaction does not filter that data.

## Limitations and Best Practices

- Prefer `Command::new` with explicit arguments. Use `Command::shell` only when shell behavior is part of the requirement.
- Configure a timeout appropriate for the command, and keep bounded output capture unless the command's behavior justifies a different policy.
- Treat timeout and cancellation output as potentially incomplete.
- Use tee files instead of unbounded memory capture for large logs.
- Keep stdin and tee paths as separate ordinary files.
- Configure cancellation from the application's existing shutdown policy; this crate does not install global signal handlers.
- Choose a timer backend that progresses independently of the synchronous caller when timeout, cancellation, or a total output budget is enabled.
- Treat captured output and tee files as untrusted process data even though diagnostics are redacted.

## Related Resources

- [README](../README.md)
- [中文 README](../README.zh_CN.md)
- [中文用户手册](user_guide.zh_CN.md)
- [API documentation](https://docs.rs/qubit-command)
- [Command-runner I/O lifecycle design](design.md)
