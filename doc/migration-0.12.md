# Migrate to qubit-command 0.12

[English user guide](user_guide.md) · [中文迁移指南](migration-0.12.zh_CN.md) · [English README](../README.md) · [中文 README](../README.zh_CN.md)

This guide covers migration from 0.11 to the managed execution APIs in 0.12. It targets `qubit-command` 0.12. Use the Cargo constraint `qubit-command = "0.12"`. Existing `CommandRunner::run`, `run_with`, and path based stdin/tee configuration remain available. Move only the call sites that need shared admission, managed process tree lifetime, or resource settlement.

## Choose an entry point

Keep the synchronous entry point for a simple one off command:

<!-- doc-example:migration-entry-selection:compile -->
```rust
use std::time::Duration;
use qubit_command::{Command, CommandRunner};

let timeout = Duration::from_secs(5);
let command = Command::new("program");
let output = CommandRunner::new(timeout).run(command)?;
```

For a service, construct one executor around its shared operation manager:

<!-- doc-example:migration-executor-setup:compile -->
```rust
use qubit_command::{CommandExecutionConfig, CommandExecutor};
use qubit_operation::{ManagerOptions, OperationManager};

let manager = OperationManager::new(ManagerOptions::default())?;
let executor = CommandExecutor::new(manager, CommandExecutionConfig::default())?;
```

Use `run_managed` or `run_managed_in` when the caller needs only the collected result. Keep the handle returned by `CommandExecutor::submit` or `submit_in` when code must inspect settlement, recovery evidence, or proof that owned files have been released.

## Process tree policy

Managed execution defaults to `ProcessTreePolicy::Managed`, regardless of timeout or cancellation configuration. Select `DirectChild` only when the application intentionally manages the direct child alone. This is a managed API choice; legacy `run` and `run_with` retain their existing lifecycle.

<!-- doc-example:migration-tree-policy:compile -->
```rust
use qubit_command::{CommandRunOptions, ProcessTreePolicy};

let options = CommandRunOptions::new()
    .with_process_tree_policy(ProcessTreePolicy::DirectChild);
```

## Managed paths and child working directory

Managed stdin, stdout tee, and stderr tee relative paths resolve from the parent process directory captured at submission. The child working directory is selected separately: a command override takes precedence over the runner default. If an old call relied on child cwd to locate a tee file, pass an explicit absolute path. Windows drive relative paths such as `C:logs\run.txt` keep Windows path semantics; they are not equivalent to `C:\logs\run.txt` and are not ordinary paths joined to the captured parent directory. The compatibility `environment` field in request snapshots remains present but empty; child environment inheritance still happens at process start, with explicit clear, remove, and override operations applied as before.

## Owned files and session stdin

Use `CommandResourceRunOptions` when transferring already opened files to one managed operation. The input starts at its current cursor; the library does not seek it implicitly. Each output sink requires an explicit `Preserve` or `Truncate` policy. Truncation occurs only during the final I/O commit after admission and startup authorization. A rejection before commit does not truncate sinks, but a partial commit can leave an earlier sink changed.

<!-- doc-example:migration-owned-file-configuration:compile -->
```rust
use std::fs::File;
use qubit_command::{CommandResourceRunOptions, FileSinkPolicy};

let input_file = File::open("input.txt")?;
let output_file = File::options().create(true).write(true).open("output.txt")?;
let resources = CommandResourceRunOptions::new()
    .stdin_file(input_file)
    .stdout_sink(output_file, FileSinkPolicy::Truncate);
```

Session stdin is a pipe and is mutually exclusive with file stdin. For a session, write through its stdin endpoint; if a fixed prefix is needed before streaming, use byte-prefix stdin where supported. Resource option values are moved because they own file handles.

## Preparation and waiting failures

Resource submission prechecks conflicting configuration before registration and validates actual opened file identities during final commit, before truncation or spawn. Owned stdin cannot be combined with command file or byte stdin; select one source. Session stdin cannot be combined with file stdin, though a supported byte prefix remains available. Tee paths may be created before a later commit error, and failure after one sink was truncated can leave that earlier change in place. File preparation failures use `CommandErrorKind::FilePreparationFailed` and `CommandErrorReason::FilePreparationFailed`, which identify the actual stage, direction, and source.

During capacity waiting, a scope stop maps to `AdmissionError::Stopped`, while deadline expiry maps to `CapacityFailure::DeadlineExpired`. A waiter already registered when its executor shuts down completes with `Capacity(Closed)`. `max_waiters` covers both local capacity waiting and manager `Busy` waiting. At a wait decision, deadline takes precedence over scope stop, which takes precedence over closure. `CommandSubmitFailureReason` now has a `Wait(CommandSubmitWaitFailure)` branch. `WaiterLimit` means the shared local waiter quota was exhausted. `Timer(RetryTimerFailure)` represents timer scheduling failure, including startup, full, or disconnected states. Callers with exhaustive matches must handle `Wait`; keep the original runner, command, and options from the failure when retrying or reporting it.

## Outcome and settlement

A managed operation has two observations: `wait_outcome()` returns the command result, while `wait_settlement()` reports resource release, transfer, or recovery required. Success does not prove helpers or owned files are released. For `RecoveryRequired`, inspect the manager report's status, effects, counts, overflow and truncation fields first, then inspect `external_settlement` for the adapter's own report and any recovery handles. Adapter recovery may already have run: its `recovery_count` can be nonzero while the manager's `recoveries` is empty. Only retry or transfer handles actually present; use settlement effects and release evidence to decide when staged files can be deleted. Collect style `run_managed*` and `run_async*` methods wait for settlement but return only the command result or stop diagnostic. Use the corresponding submit handle when settlement evidence is needed.

An accepted stop before process start is observed through an operation handle as `OperationOutcome::Stopped`; after settlement, call `take_stop_diagnostic()` once on that concrete command handle. `None` is possible. Collecting `run_managed*` methods keep their existing `CommandError` mapping. Treat operation observer or settlement errors as lifecycle observation failures, not as a command exit status.

| Previous assumption | 0.12 guidance |
| --- | --- |
| `run` and `run_with` must change | Keep them unless managed admission or settlement is needed. |
| Managed tee paths follow child cwd | Pass an absolute path or resolve from the parent directory. |
| Command success means files are ready for deletion | Wait for settlement and recover when required. |
| Every submit failure is capacity or admission | Handle the new `Wait` reason and inspect its typed cause. |
