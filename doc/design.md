# Qubit Command Design

[中文版](design.zh_CN.md) · [User Guide](user_guide.md) · [README](../README.md)

This document records the `qubit-command` 0.8 command-runner contract. It is
for maintainers and callers that need to reason about process ownership, I/O
helpers, termination, and the information preserved in failures. The user
guide remains the entry point for runnable examples.

## Purpose

`CommandRunner` turns an external process into one bounded application step.
The runner owns the child process, its stdout/stderr readers, and an optional
stdin writer until normal completion or a terminal error. It keeps process
control, stream collection, output limits, cancellation, timeout, and
diagnostic redaction explicit. It does not install signal handlers, define a
shell language, cache command output, or make captured process data safe.

## Public Contract

- `Command` describes a program, structured arguments, environment and working
  directory changes, and stdin. `Command::new` does not invoke a shell;
  `Command::shell` explicitly selects `sh -c` on Unix-like systems or `cmd /C`
  on Windows.
- `CommandRunner` supplies timeout, successful-exit-code, output-capture,
  logging, timer, and redaction policy. `run` is synchronous; `run_with`
  accepts per-run options such as cancellation and tee files.
- `CommandOutput` carries exit status, retained stdout/stderr bytes, elapsed
  time, truncation flags, and `stdout_complete()`/`stderr_complete()`.
- `CommandError` exposes a stable `kind()` classification and optional output.
  `output()` borrows retained output and `into_output()` moves it out. Cleanup
  failures are available through `cleanup_failures()` and do not replace the
  primary error.

The default captures at most 1 MiB per output stream and treats exit code zero
as success. `stdin_file` and tee paths must identify ordinary files. A tee is
replaced at the beginning of a run, not appended. Callers that use
`unbounded_output()` accept unbounded in-memory process data themselves.

## Ownership States

The runner moves through these ownership states:

1. **Prepared:** command values and file paths are validated. Input/tee path
   conflicts and non-regular files fail before a child is spawned or a tee is
   truncated.
2. **Started:** the child and all requested I/O helpers have been created.
   The child process, reader threads, writer thread, cancellation handles, and
   timer are owned by one running-command state.
3. **Observed:** the child has exited, timed out, or cancellation was observed.
   The runner still owns the child status and helper results while it applies
   the completion policy.
4. **Finalized:** every helper that can be joined within the contract has been
   joined and its result has been folded into `CommandOutput` or cleanup
   diagnostics. On Windows, a failed helper cancellation has a bounded
   confirmation window; an unconfirmed helper may be detached and finish when
   its pipe closes.

The final startup cancellation check is the linearization point. Cancellation
before it produces `CancelledBeforeStart` without creating or truncating tee
files. Cancellation after it is an in-flight cancellation and may retain
partial output.

## Run Events

The normal event order is:

1. Validate command, file kinds, path conflicts, and cancellation state.
2. Spawn the child and configure process-tree management.
3. Start stdout/stderr readers and the optional stdin writer.
4. Record the post-spawn monotonic start instant and poll child status,
   timeout, and application cancellation.
5. On ordinary exit, join helpers and assemble output.
6. On timeout or cancellation, terminate the managed process tree, request
   helper cancellation, collect available bytes, and classify the primary
   policy result.

The timeout starts after spawn. A timer must continue progressing while the
synchronous `run` call blocks. A cancellation handle is one-shot and
idempotent; repeated `cancel()` calls do not create additional transitions.

## Termination

When process-tree management is available, the runner first requests tree
termination. If that request fails, it checks whether the child already exited,
then tries direct-child termination. The original tree-termination source is
retained as a cleanup failure when the child status can still be confirmed.
Wait and kill errors are mapped without discarding earlier termination
evidence. If the final child status cannot be confirmed, the initiating stop reason
remains primary and all process-control failures remain cleanup evidence.

Termination is best effort with respect to descendants and external effects.
The runner does not promise to undo work already performed by a child or by a
descendant that escaped the managed process tree.

Startup errors call the same termination policy explicitly before returning. Both
failed kill requests with unknown status never lead to a blocking wait. OS wait
after an accepted kill and ordinary-file I/O are not hard-deadline operations.

## I/O Finalization

Each output reader and optional stdin writer owns a cancellation token and a
join handle. All cancellation requests are issued before waiting on any one
helper, so one failed request cannot prevent the others from being attempted.
On ordinary completion, helper results are joined and stream bytes are
returned. On interruption, available bytes are retained and the corresponding
`*_complete()` flag is false when the helper did not drain its pipe to EOF.

On Unix, readers and stdin writers use non-blocking operations plus a wake-up
channel so cancellation can be observed without waiting for an escaped
descendant to close an inherited pipe. On Windows, synchronous operations are
interrupted through `CancelSynchronousIo`. If a Windows cancellation request
fails, the runner waits no longer than 100 ms for the helper to report that it
stopped. After that deadline it returns a bounded result with a
`StdoutCancellation`, `StderrCancellation`, or `StdinCancellation` cleanup
failure as applicable; the helper may remain alive until its pipe closes.

Output truncation is independent of stream completeness. A complete stream can
be truncated in memory, and an incomplete stream can contain fewer bytes than
the configured limit. Callers must check both kinds of metadata before treating
retained bytes as a complete transcript.

## Error Precedence

The primary error describes the first decisive failure in the operation's
policy. Secondary failures observed while terminating the child or finalizing
helpers are retained in `cleanup_failures()` in deterministic helper order.
The order is process tree, direct child, wait, clock, stdout, stderr, stdin.
`Time` retains clock failures observed during finalization. Demoting a helper
error also moves its retained output into the primary error. `KillFailed` and
`CancelFailed` are removed; timeout and cancellation never change primary kind
because cleanup failed.

Preparation, thread-start, timer, and process-control failures may not carry a
`CommandOutput`. Timeout, cancellation, truncation, unexpected exit, output
read, tee-write, and final stdin-write failures carry output whenever status,
elapsed time, and stream state can be assembled. A cleanup failure does not
make partial output complete and does not imply that a descendant was stopped.

## Platform Semantics

Unix uses process groups and non-blocking pipe polling. Path-backed stdin and
tee files are opened with non-blocking safety flags, checked through the live
handle, then restored to blocking mode before use. This prevents a FIFO path
replacement from blocking preparation.

Windows uses Job Objects for process-tree management where configured and
keeps pipe handles suitable for synchronous I/O. `CancelSynchronousIo` is the
normal helper interruption mechanism. Its failure is observable through the
typed cancellation cleanup failure and the 100 ms confirmation rule above;
the rule is a bounded-return guarantee, not a promise that the operating
system closes a pipe immediately.

Other platforms perform handle-authoritative ordinary-file checks but cannot
portably guarantee that every device-namespace open returns promptly. Broken
remote or FUSE filesystems can still stall an operating-system operation; such
stalls are outside the command timeout contract.

## Testing Seams

The runner accepts an injected timer and clock, which lets tests advance time
without sleeping through a real timeout. Process-control wrappers, file
preparation, output readers, tee writers, cancellation tokens, and cleanup
failure mapping are tested at their seams. Integration tests also exercise
real commands, shell selection, non-UTF-8 values, ordinary-file rejection,
process-tree cleanup, cancellation, timeout, truncation, tee output, and
redacted diagnostics.

## Coverage Gates

Changes to lifecycle behavior must retain tests for normal exit, timeout,
startup cancellation, in-flight cancellation, helper cancellation failure,
partial output, cleanup-error precedence, and both Unix and Windows-specific
paths where the platform APIs differ. Documentation examples are compiled as
doctests or by the repository Markdown checker. Package validation must include
both README files, both user guides, and both design documents, and must not
depend on files excluded from the Cargo archive.

## Known Limits

- The library cannot validate the semantics of a target program's arguments or
  shell script; callers own input validation.
- Process-tree termination and helper interruption depend on operating-system
  capabilities. Descendants can keep inherited pipes open, and a Windows
  helper can outlive the bounded confirmation window.
- Captured output and tee files are raw process data even when diagnostics are
  redacted. Callers must apply their own retention and access policy.
- Timeout excludes preparation and spawn. It does not bound arbitrary kernel
  operations on untrusted or remote filesystems.
- Any future public lifecycle change must update the error contract, stream
  completeness semantics, both language guides, and this design document.
