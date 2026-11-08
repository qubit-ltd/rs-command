# Windows Cancellation Status

**Status: unresolved.** The runner's successful-cancellation path can join a helper without a deadline. The inspected production paths and available probes do not establish that a successful cancellation is followed by blocking production I/O that keeps this join pending. This review neither confirms nor rules out a production hang, and it does not prove Windows safety.

## What the contract says

On Windows, successful `CancelSynchronousIo` means that the cancellation request succeeded; it does not mean the helper has stopped. The runner establishes one shared 100 ms helper-confirmation deadline before cancellation requests. It consults that deadline only when a cancellation request fails, and does not reset it for each helper. A successful request proceeds to join without that deadline. The 100 ms window is not a hard deadline for the whole `run()` call.

Callers should preserve the primary error and its `cleanup_failures()`, then check `stdout_complete()` and `stderr_complete()` before treating retained output as complete. Applications that require a hard return deadline should supervise the runner in a separate process boundary; the runner's timeout does not provide that guarantee.

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
