# RC-002 Windows Cancellation Interleaving Verification

## Status

**Not completed: a Windows runner is required to verify the reported race.**
This investigation found plausible interleaving windows in the source, but no
Windows reproduction or equivalent operating-system evidence was available in
the local environment. No production code was changed for RC-002.

## Environment

- Host: Linux x86_64 (`Linux 6.8.0-147-generic`)
- Windows execution: unavailable locally
- Relevant repository CI: `.github/workflows/ci.yml` includes `windows-2022`
- Local verification command: `.infra/bin/ci-check.sh` (passed on Linux;
  this is not Windows evidence)

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

No controlled reader/tee/stdin interleaving test was added because a fake
worker that ignores cancellation would only prove the join strategy, not the
Windows operating-system race. The required Windows run should exercise a
barrier-controlled read-to-tee, EOF-to-flush, and stdin check-to-write schedule,
with a watchdog and release path, and record the exact test names and results.

## Decision

No confirmed contract violation has been established, so the conditional repair
branches were not applied. RC-002 remains open until the controlled Windows
interleaving is run. If it reproduces a helper continuing into new I/O after a
successful cancellation request, repair only that path and retain the existing
error precedence, captured bytes, EOF state, and cancellation cleanup contract.
