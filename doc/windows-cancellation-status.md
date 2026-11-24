# Windows Cancellation Status

**Status: unresolved.** The runner's successful-cancellation path can join a helper without a deadline. The inspected production paths and available probes do not establish that a successful cancellation is followed by blocking production I/O that keeps this join pending. This review neither confirms nor rules out a production hang, and it does not prove Windows safety.

The `qubit-command` 0.12 managed API adds process-tree ownership and a separate settlement report; it does not change this helper-join evidence or establish a hard return deadline on Windows. A `Managed` process-tree policy must not be read as proof that every helper join is bounded. The same limitation applies to managed commands; inspect settlement and cleanup reports, and use a separate process boundary when the application requires a hard deadline.

## Integrated source audit at SHA `66e901499fb6fde370ce02bf6cf50e458addfefb` (2026-11-24)

This source-only audit compared the integrated production source at `66e901499fb6fde370ce02bf6cf50e458addfefb` with the historical native Windows source SHA `c871a25024adb72c1f4bdac7f9e226fb03463cb8`. It ran on Linux x86_64 (`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`) with Rust `1.94.0` and Cargo `1.94.0`. `pwsh` and `powershell` are unavailable, so this SHA has no native Windows rerun, PowerShell gate run, or new trace.

**Cancellation and helper:** The integrated `RunningCommand`/startup path changed. `RunningCommand` now observes managed external cancellation through `StartGate`, carries operation cancellation state, and propagates timeout stop to the gate. This connects the managed operation stop decision to the runtime loop. The helper implementation in `command_io.rs`, including `CommandIo::cancel_and_join` and the Windows native cancellation path, is unchanged from `c871a250`. Successful cancellation therefore still proceeds to helper join without a deadline; the shared 100 ms helper-confirmation deadline remains on the cancellation-failure path. This source comparison establishes neither a pending join nor a production hang.

**Cleanup and reaper:** T2 changed startup-abort and runtime cleanup handoff, reaper service initialization/replacement, and managed recovery. Handoff failures now distinguish a full bounded queue from a disconnected or failed service; a disconnected pool is replaced and retried. Full queues retain the cleanup owner for recovery retry. Direct startup/runtime handoff paths run cleanup inline when the service cannot start or remains disconnected. If a managed deferred owner already retains the cleanup, it preserves ownership on retry failure; its designated recovery-worker fallback may run cleanup inline for an unavailable service, while a full queue remains retryable. The managed recovery worker counts queued and pending owners against its capacity, uses bounded backoff, advances independent owners without waiting on one owner's settlement, and publishes release/settlement only after cleanup completion. These are material integrated cleanup changes, but they do not change the helper's native cancellation or prove the Windows I/O sequence under investigation.

**Managed file preparation:** T4 changed managed stdin and tee preparation/commit paths, including direction/stage classification, identity checks against managed handles, and truncation error classification. These changes alter how the integrated managed operation reaches and cleans up file-backed I/O; they were not exercised by the historical native run.

**Probe, workflow, and identity gate:** A targeted `git diff --exit-code c871a25024adb72c1f4bdac7f9e226fb03463cb8 HEAD --` over `windows_probe_tests.rs`, `windows-cancellation-interleaving.yml`, and both `assert-windows-probe-results` PowerShell scripts exited 0. They are unchanged from the native SHA. The existing workflow still targets `windows-2022`, pins Rust `1.94.0`, runs the Windows and command-I/O filters, records selected test counts and `GITHUB_SHA`, executes identity-gate fixtures, and applies the exact-name result gate for 12 required tests. The existing probe still does not establish a subsequent blocked production I/O or a cleanup join held by that I/O.

**Decision:** RC-006 remains **unresolved** at integrated source SHA `66e901499fb6fde370ce02bf6cf50e458addfefb`. The required same-run, same-production-helper chain remains unproven: (1) native cancellation succeeds; (2) the original I/O returns; (3) a subsequent production I/O enters and remains blocked; (4) cleanup join is observed waiting on that I/O; (5) an independent watchdog releases it; and (6) helper/process/resource cleanup is then observed complete. The native run at `c871a250` remains historical evidence only. No current-SHA Windows run, PowerShell identity gate, or trace was produced, and no production cancellation code or probe was changed.

## Post-integration source audit at SHA `a8fab3aed3a0999b6049dcf344d2e075d63e0ea5` (2026-11-24)

This bounded audit ran on Linux x86_64 (`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`) with Rust `1.94.0` and Cargo `1.94.0`. A targeted `git diff --exit-code c871a25024adb72c1f4bdac7f9e226fb03463cb8 --` over the production helper cancellation/join, Windows probe, workflow, and PowerShell identity-gate paths exited 0. `command -v pwsh` and `command -v powershell` each exited 1. No Windows runtime test, PowerShell identity gate, or new trace was run for this SHA.

The integrated entry path now observes the managed operation start gate and passes its cancellation handle into `RunningCommand`; managed-file preparation and commit paths also changed. The native helper cancellation implementation and `cancel_and_join` path did not change in the targeted comparison: Windows cancellation success still proceeds directly to join, and the shared 100 ms confirmation deadline is still used only after cancellation failure. The Windows probe, workflow, and identity gate are unchanged from the recorded native run at `c871a250`. The run at that earlier SHA remains evidence only for that source revision; it does not exercise the post-integration entry and file-handoff paths.

**Decision:** RC-006 remains **unresolved**. No same-run trace at the integrated source SHA establishes the required production causal chain, and the source comparison does not confirm or exclude a production hang or establish Windows safety. No production cancellation code or probe was changed. The existing `windows-2022` workflow and PowerShell identity gate remain the runtime evidence entry points; final remote evidence must be tied to the exact pushed source SHA.

## What the contract says

On Windows, successful `CancelSynchronousIo` means that the cancellation request succeeded; it does not mean the helper has stopped. The runner establishes one shared 100 ms helper-confirmation deadline before cancellation requests. It consults that deadline only when a cancellation request fails, and does not reset it for each helper. A successful request proceeds to join without that deadline. The 100 ms window is not a hard deadline for the whole `run()` call.

Callers should preserve the primary error and its `cleanup_failures()`, then check `stdout_complete()` and `stderr_complete()` before treating retained output as complete. Applications that require a hard return deadline should supervise the runner in a separate process boundary; the runner's timeout does not provide that guarantee.

## Bounded source audit at SHA `36579c289a9cd6e9fb981854e87a7e3137cb6512` (2026-11-23)

This audit ran on Linux x86_64 with Rust `1.94.0`. A targeted `git diff --exit-code fcdccb7 --` over the production cancellation/helper, Windows probe, workflow, and identity-gate paths exited 0: those paths are unchanged from the latest recorded source audit. `command -v pwsh` and `command -v powershell` each exited 1. No Windows runtime test, PowerShell identity gate, or new Windows trace was run for this SHA.

The existing production-path probe still does not witness a subsequent blocked production I/O or a production cleanup join waiting on it: its recorded fields include `original_io_return_is_subsequent_io=false`, `production_join_pending_witness=false`, `source_writer_manually_released=true`, and `watchdog_released=false`. The pipe-based interleaving probes and separate watchdog self-test cannot establish the ordinary-file tee or production-stdin causal chain. The existing `windows-2022` workflow and PowerShell identity gate remain the runtime and result-interpretation entry points, but neither was available in this environment.

**Decision:** RC-006 remains **unresolved**. This source comparison confirms only that the inspected paths did not change since `fcdccb7`; it neither confirms nor excludes the suspected production hang and does not establish Windows safety. No production code or probe was changed.

## Native Windows workflow evidence at SHA `c871a25024adb72c1f4bdac7f9e226fb03463cb8` (2026-11-24)

The [Windows Cancellation Interleaving run](https://github.com/qubit-ltd/rs-command/actions/runs/45305695014) completed successfully on `windows-2022` (Windows Server 2022). It selected 14 tests: 12 `windows_` tests and 2 `command_io` tests; all passed. The PowerShell identity-gate fixtures passed, and the result gate reported all 12 required tests as discovered and passed. The [probe artifact](https://github.com/qubit-ltd/rs-command/actions/runs/45305695014/artifacts/14880568265) contains the run, test-list, command-I/O, and gate logs.

The same-run production-path record for `production_command_io_blocked_read` observed a successful native cancellation (`cancel_success=true`), then return from the original blocked read with raw OS error `995`, followed by cleanup join, helper join, and child reaping. The record also states `original_io_return_is_subsequent_io=false`, `production_join_pending_witness=false`, `tee_write_entries=0`, `source_writer_manually_released=true`, and `watchdog_released=false`; its classification remains `unresolved`. Thus the trace did not observe a post-cancellation production tee write/flush or stdin write entering and remaining blocked, nor a production cleanup join held by such later I/O until independent watchdog release.

The separate EOF/flush and readiness probes had zero valid hits. The readiness pipe probe recorded 128 successful cancellations but zero confirmed blocked writes. The stdin check-to-write probe observed its in-flight write return with raw OS error `995` and zero post-cancel write entries. These probe results do not establish the production-file or production-stdin causal chain. The separate watchdog self-test reaped its child, but did not witness or release a production cleanup join.

**Decision:** RC-006 remains **unresolved**. This native run passed its selected tests and identity gate, but it did not produce the required same-helper sequence of successful cancellation, original I/O return, subsequent blocked production I/O, cleanup join waiting on that I/O, independent watchdog release, and recovery. The earlier Linux source audit at `36579c2` remains historical evidence only.

## Bounded source audit at SHA `dec767dff01f9fb6c97c923628d472f9a81f33cd` (2026-11-24)

This audit ran on Linux x86_64 (`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`) with Rust `1.94.0` and Cargo `1.94.0`. A targeted `git diff --exit-code c871a25024adb72c1f4bdac7f9e226fb03463cb8 --` over the production cancellation/helper, Windows probe, workflow, and identity-gate paths exited 0. `command -v pwsh` and `command -v powershell` each exited 1. No Windows runtime test, PowerShell identity gate, or new Windows trace was run for this SHA.

The inspected production path still sends successful Windows cancellation to helper join without the confirmation deadline. The production probe reports `production_join_pending_witness=false`, `original_io_return_is_subsequent_io=false`, `source_writer_manually_released=true`, and `watchdog_released=false`; the pipe probes do not establish the production-file or production-stdin causal chain. The existing `windows-2022` workflow remains the viable runtime entry point: it pins Rust `1.94.0`, runs the Windows and command-I/O test filters, and applies the PowerShell identity gate. It was not dispatched for this audit.

**Decision:** RC-006 remains **unresolved**. This source comparison confirms only that the inspected paths match the recorded `c871a250` run; it neither confirms nor excludes the suspected production hang and does not establish Windows safety. No production code or probe was changed, and no artifact was generated.

## Bounded source follow-up (source SHA `5fa0cae06001e07b5053171216b1b9f7da6acb15`, 2026-11-18)

The implementation-stage source check ran on Linux x86_64 with Rust `1.94.0`. The retry-timer test change in `src/execution/executor.rs` does not alter Windows cancellation, helper I/O, or join paths; the stop-diagnostic change is documentation-only. A targeted `git diff --exit-code fcdccb7 --` over the production cancellation/helper, Windows probe, workflow, and identity-gate paths exited 0. `pwsh` and `powershell` were unavailable, so no Windows runtime test, PowerShell gate, or new Windows trace was run.

No new production-reachable cancellation/interleaving assumption was found and no conditional repair was triggered. RC-006 remains **unresolved**: this source comparison neither confirms nor excludes the suspected hang and does not establish Windows safety. See the [verification history](rc-002-verification.md#current-summary) for prior evidence and its limits.

## Current implementation source audit at SHA `fcdccb7` (2026-11-18)

This bounded audit ran on Linux x86_64 with Rust `1.94.0`. A targeted
`git diff --exit-code 23085b070bd42c7e77c14f22390a7df7eafae767 -- <production cancellation/cleanup, output, stdin, Windows probe, workflow, and identity-gate paths>`
exited 0. Source reads and `rg -n` checks covered `command_io.rs`,
`io_cancellation.rs`, `stdin_pipe.rs`, `output_collector.rs`, and the updated
session channel stop/backpressure paths. Both `command -v pwsh` and
`command -v powershell` exited 1. No Windows runtime test, PowerShell identity
gate, or new Windows trace was run for this SHA.

The production cancellation control flow remains as described below: Windows
`CancelSynchronousIo` can succeed before the output-reader or stdin-writer join
returns. The output reader checks cancellation after a read and before tee I/O;
the production tee is a regular file. Stdin checks cancellation before each
readiness/write cycle. This implementation changes the managed-session byte
channel to preserve queued bytes under capacity backpressure and wake blocked
writers when output is stopped. That user-space channel notification is
separate from native Windows synchronous-I/O cancellation and does not change
the evidence about helper joins.

**Decision:** no new production-reachable interleaving hypothesis was found.
RC-006 remains **unresolved**; this source audit neither confirms nor excludes
a production hang and does not establish Windows safety. No Windows-specific
code or probe was changed.

## Previous implementation source audit at SHA `e8c7e8a0fc2a4da33b85ab98d172f9aca1112fa8` (2026-11-17)

This bounded audit ran on Linux x86_64 with Rust `1.94.0`. A targeted source comparison against the review baseline `fa506a6fb75bc6bcfd3537c5be54b4cda911815e` exited 0 for the production cancellation/cleanup, output, stdin, Windows probe, workflow, and identity-gate paths. Neither `pwsh` nor `powershell` is installed (each `command -v` exited 1). No Windows runtime test, PowerShell identity gate, or new Windows trace was run for this SHA.

| Event | Source observation | Evidence limit |
| --- | --- | --- |
| Cancellation and join | Windows `IoCancellation::cancel` publishes the cancellation flag before calling `CancelSynchronousIo`. Cleanup requests cancellation for stdout, stderr, and stdin before finishing helpers. A successful request proceeds directly to the corresponding join; the shared 100 ms confirmation deadline is consulted only after a failed cancellation request. | The successful-cancel join is unbounded, but source control flow alone does not show that it remains pending. |
| Output read and tee | The reader retains bytes returned by a completed read, checks cancellation, and can then synchronously write to the tee. The production tee is a regular `File`; EOF can lead to `flush`. | A check-to-I/O scheduling gap exists, but no same-run evidence shows successful cancellation followed by blocked production I/O and a pending production join. |
| Stdin | The helper checks cancellation before each readiness/write cycle. On Windows, readiness checks the cancellation token. | No production-reachable sequence of successful cancellation followed by a blocked write was established. |
| Probe and watchdog | The production probe records `original_io_return_is_subsequent_io=false`, `production_join_pending_witness=false`, `source_writer_manually_released=true`, and `watchdog_released=false`. The blocked-pipe watchdog self-test is a separate trace. | Existing evidence does not connect successful cancellation, subsequent blocked production I/O, a pending cleanup join, independent watchdog release, and helper/process recovery in one trace. |

**Decision:** no new production-reachable interleaving hypothesis was found, so no conditional production repair was triggered. RC-006 remains **unresolved**: the concern is neither confirmed nor excluded, and this source audit does not establish Windows safety. No production code or probe was changed.

## Evidence at source SHA `76fda78d3c4e8961a21193f749bf1228b50929a4`

This bounded source review ran on Linux x86_64 with Rust `1.94.0`. The source comparison against the recorded SHA exited 0 with no changes in the inspected production, probe, workflow, or identity-gate paths. `pwsh` and `powershell` are unavailable; an installed Windows GNU target does not provide a Windows runtime. No Windows test, PowerShell gate, or new Windows trace was run for this SHA.

| Path | Confirmed control flow | Evidence limit |
| --- | --- | --- |
| Cancellation and cleanup | `IoCancellation::cancel` publishes the token before the Windows `CancelSynchronousIo` call. `finish_helpers_with_wait` requests cancellation for stdout, stderr, and stdin before finishing helpers. Successful cancellation reaches the output-reader or stdin-writer join directly; the shared deadline is for failed cancellation. | Confirms the unbounded successful-cancel join, not that it blocks. |
| stdout/stderr and tee | `read_output_inner` checks cancellation after a read and before a tee write. The production tee is an ordinary `File`; the interleaving probe uses an intentionally filled anonymous pipe. | The probe's pipe does not establish blocking behavior for the production file path. No same-run trace connects successful cancellation to later blocked production I/O and a pending production join. |
| stdin | `write_stdin_until_cancelled` checks cancellation before each readiness/write cycle. The Windows probe's filled pipe is not the production `ChildStdin` path. | No production-reachable successful-cancel-then-blocking-write sequence was established. |
| Probe and watchdog | Existing probe evidence does not combine successful cancellation, a subsequent blocked production operation, a cleanup join pending because of that operation, independent watchdog release, and helper/process recovery in one trace. | Test identity checks and a separate watchdog self-test cannot establish that causal chain. |

**Decision:** no new production-reachable interleaving hypothesis was found, so the conditional T3 regression/repair was not triggered. RC-002 remains unresolved; no production code or probe was changed. This conclusion is limited to the inspected paths and schedules.

## Verification record

- `./.infra/bin/align-ci.sh` — exit 0; formatting/alignment completed successfully.
- `git diff --exit-code 76fda78d3c4e8961a21193f749bf1228b50929a4 -- src/command_runner/internal/command_io.rs src/command_runner/internal/io_cancellation.rs src/command_runner/internal/io_cancellation_token.rs src/command_runner/internal/cancel.rs src/command_runner/internal/cancellable_reader.rs src/command_runner/internal/output_collector.rs src/command_runner/internal/output_reader.rs src/command_runner/internal/stdin_pipe.rs src/command_runner/internal/stdin_writer.rs src/command_runner/internal/pollable_stdin.rs src/command_runner/internal/windows_cancel_interleaving_tests.rs src/command_runner/internal/output_collector/tests/windows_probe_tests.rs .github/workflows/windows-cancellation-interleaving.yml .github/scripts/assert-windows-probe-results.ps1 .github/scripts/assert-windows-probe-results.tests.ps1` — exit 0; no changes against the review baseline.
- `./.infra/bin/style-check.sh` — exit 0.
- `cargo check --workspace --all-targets` — exit 0.
- `./.infra/bin/ci-check.sh` — exit 0; project tests, doctests, documentation examples, feature matrix, package verification, and coverage passed. Coverage: functions 96.95%, lines 96.82%, regions 96.74%.
- `cargo package --list --allow-dirty` — exit 0; both bilingual status pages are included through the existing `/doc/**` package rule.
- `uname -a` — exit 0; Linux x86_64.
- `rustc --version` — exit 0; `rustc 1.94.0 (4a4ef493e 2026-03-02)`.
- `command -v pwsh` and `command -v powershell` — each unavailable (exit 1); Windows-only test commands and the PowerShell identity gate were not run.
- `cargo test --lib windows_ -- --show-output --test-threads=1 --color never` and `cargo test --lib command_io::tests::test_error_not_found_ -- --show-output --test-threads=1 --color never` — not run because this host is Linux and no Windows runner was available in the local environment.

## History and update criteria

See the [RC-002 verification history](rc-002-verification.md#current-summary) for earlier audits and Windows runs, each tied to its own source SHA and environment. A future status change requires a same-run, same-helper ordered trace that connects the actual production path, successful native cancellation, return from the cancelled operation, entry into and witness of a subsequent blocking production I/O, a production cleanup join waiting on it, independent watchdog release, and helper/process recovery. Probe injection must not create an operation unavailable on the production path. A failed or unreproduced experiment alone cannot resolve the concern.

中文摘要：[Windows 取消状态](windows-cancellation-status.zh_CN.md)。
