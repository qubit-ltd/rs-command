# RC-002 Windows Cancellation Interleaving Verification

## Status

**Not completed: the controlled cancellation interleaving remains unverified.**
The source contains plausible interleaving windows. The active implementation
host is Linux x86_64, but GitHub Actions was available for the commit. Its
Windows 2022 job passed the existing CI suite; the repository did not contain
the planned `windows_successful_cancel_interleaving` test, so that run does not
establish whether the reported race occurs. No production code was changed for
RC-002.

## Environment

- Host: Linux x86_64 (`Linux 6.8.0-147-generic`)
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`
- Baseline commit: `d09a6624f10790d2bc0b171c919ee18f83105586`
- Direct Windows execution from the active workspace: unavailable
- Relevant repository CI: `.github/workflows/ci.yml` includes `windows-2022`,
  and was used for the committed changes.
- GitHub Actions run: [Rust CI run 41689180683](https://github.com/qubit-ltd/rs-command/actions/runs/41689180683),
  commit `ab636a295c717fa3bbd2f23dc5ac95614aabf06b`; `rust-ci / verify
  (windows-2022)` completed successfully on 2026-11-03.
- Planned Windows interleaving command:
  `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`
  was not run because that test is not present in the repository. The Windows
  CI job ran existing tests and checks, not the planned controlled interleaving.

## Source Trace

- `internal/io_cancellation.rs::IoCancellation::cancel` sets the cancellation
  token before requesting platform cancellation.
- On Windows, `internal/cancel.rs::cancel_synchronous_io` calls
  `CancelSynchronousIo`. A successful call returns `Ok(())`; error 1168
  (`ERROR_NOT_FOUND`) and other errors remain distinguishable.
- `internal/command_io.rs::finish_helpers` requests cancellation for stdout,
  stderr, and stdin before finishing helpers. It uses one shared 100 ms
  confirmation deadline only when a cancellation request returned an error.
  A successful request proceeds directly to `join`, which has no deadline.
- `internal/output_collector.rs::read_output_inner` checks cancellation before
  the next readiness/read operation. After a successful read it retains bytes,
  then may write them to the tee before checking the token again at the next
  loop iteration. EOF is recorded before the optional tee flush; `complete`
  therefore remains true if cancellation arrives after EOF.
- `internal/stdin_pipe.rs::write_stdin_until_cancelled` checks cancellation,
  calls `wait_writable`, and then writes. The Windows `wait_writable`
  implementation checks the token, leaving a check-to-write interval.

These source paths identify the interleavings to exercise. They do not establish
that Windows `CancelSynchronousIo` can return success while the operation
completes normally and the helper proceeds into a later synchronous I/O call.
That operating-system behavior is the unresolved point.

## Test Evidence and Limits

The Linux CI run passed the existing `command_io` and cancellation tests,
including `test_finish_helpers_joins_when_all_cancellations_succeed`. That test
uses already completed helpers and does not cover a live Windows synchronous
I/O completion race. Existing cancellation-failure tests likewise do not
substitute for the successful-cancellation interleaving.

No controlled reader/tee/stdin interleaving test was added. A fake worker that
ignores cancellation would only prove the join strategy, not the Windows
operating-system race. The successful Windows CI run above had no test that
exercised barrier-controlled read-to-tee, EOF-to-flush, or stdin check-to-write
schedules with a real `CancelSynchronousIo` result and watchdog release path.
Those experiments and their exact test results remain outstanding.

## Current Investigation Attempt

- Worktree branch: `codex/rc001-rc002-implementation-20261103`, based on
  `9f0f306`; RC-001 was committed as `10f0ac0`. The task branch has no matching
  ref on `origin` yet.
- The active host is Linux x86_64. The repository's Windows 2022 GitHub Actions
  workflow supports `workflow_dispatch`, so a branch run is available after
  the controlled test exists and the branch is published. No such run was
  dispatched because the required interleaving test is not present. The
  earlier Windows job therefore remains unrelated to this test.
- The repository has no controlled Windows interleaving test or barriers at
  the read-to-tee, EOF-to-flush, or stdin check-to-write boundaries. Adding a
  test that merely holds a helper after a successful cancellation would test
  the join policy, not whether Windows cancellation can be followed by a new
  synchronous I/O call. I did not add such a test or claim it as evidence.
- There is no existing test seam that can establish the required OS state.
  Pausing after `Read::read` returns, after EOF, or after `wait_writable`
  returns means the worker is between synchronous I/O calls; a cancellation
  request at that point cannot be evidence that `CancelSynchronousIo` returned
  success for the preceding operation. Pausing before the next call and then
  setting the token also makes the helper's normal cancellation checks stop it
  before entering that call. Reaching the required schedule therefore needs a
  race between a real Windows I/O completion and `CancelSynchronousIo`, with
  instrumentation that records the actual system result and whether the next
  real I/O call began. No such instrumentation or watchdog/release harness is
  present. A test-only blocking fake would bypass the OS behavior under review
  and was not used.
- A best-effort stress loop over real pipes would be probabilistic: it cannot
  guarantee that the completion/cancellation race occurs, and zero hits would
  leave the same evidence gap. Introducing scheduler barriers cannot make that
  schedule deterministic for the reasons above. No such probe was added; the
  test design must not report a pass unless it observes a valid schedule.
- No Windows process was available locally. The Windows CI runner can execute
  a branch test once implemented and published, but the exact planned Windows
  commands remain unrun. No event trace, watchdog result, or helper-reap result
  was collected for these schedules.

## Current Implementation Validation

The implementation branch is `codex/rc001-rc002-implementation-20261103` at
`10f0ac0` for the completed RC-001 commit. On Linux x86_64 with
`rustc 1.94.0 (4a4ef493e 2026-03-02)`, the following completed successfully:

- `.infra/bin/align-ci.sh` and `.infra/bin/style-check.sh`
- `cargo test --lib budget_only_timer` (2 passed, 89 filtered)
- `cargo test --all-targets` (exit code 0)
- `cargo test --doc` (15 passed)
- `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps`
- `.infra/bin/ci-check.sh` (exit code 0, including style, clippy, verify,
  strict docs, feature matrix, package, coverage, and audit)

These checks validate RC-001 and the Linux project baseline. They do not
resolve RC-002 or substitute for its Windows interleaving evidence.

## Decision

No confirmed contract violation has been established, so the conditional repair
branches were not applied. RC-002 remains open until the controlled Windows
interleaving is run. If it reproduces a helper continuing into new I/O after a
successful cancellation request, repair only that path and retain the existing
error precedence, captured bytes, EOF state, and cancellation cleanup contract.

The current result is **unresolved because a valid schedule was not executed**;
it is neither a successful race check nor evidence that the race is absent.
