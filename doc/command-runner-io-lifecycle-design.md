# Command Runner I/O Lifecycle Design

## Purpose

`qubit-command` treats command I/O as part of the command lifecycle. Timeout and
cancellation cleanup must not detach library-owned helper threads, and returned
output must state whether each stream was completely drained.

## Ordinary-file boundary

`Command::stdin_file`, `CommandRunOptions::tee_stdout_to_file`, and
`CommandRunOptions::tee_stderr_to_file` accept ordinary files only. Existing paths
are checked through their metadata as an early classification aid; a missing tee
path may be created as a regular file. The metadata read from the opened handle
is authoritative for every stream, so a path replacement between the early check
and open cannot make a FIFO, device, socket, directory, or other special file
usable. A symlink is accepted only when its target is an ordinary file.

On Unix, all path candidates are opened with `O_NONBLOCK`, checked from the live
handle, and restored to blocking mode before the handle is passed to the child or
tee writer. Thus a FIFO replacement cannot block command preparation. Windows and
other non-Unix targets perform the same handle-authoritative check, but have no
portable way to guarantee that an arbitrary device-namespace open itself returns
promptly; trusted ordinary-file paths are required for that stronger property.
Broken remote or FUSE filesystems may still stall an operating-system operation,
which remains outside the command timeout contract.

The output collector preserves a `CommandOutput` for output-read and final
stdin-write failures whenever process status, elapsed time, and both stream
states can be assembled. Preparation, thread-start, clock, and process-control
failures may not carry output; callers use `CommandError::output()` or
`CommandError::into_output()` as the single access path.

## Helper lifecycle

Each stdout reader, stderr reader, and buffered stdin writer owns a cancellation
flag and a join handle. On normal completion the handle is joined normally. On
timeout, cancellation, startup failure, or timer failure the runner first
requests cancellation and then joins every helper. No helper is detached.

Unix child pipes use non-blocking loops so cancellation can be observed without
waiting for an escaped descendant to close an inherited pipe. Windows helpers
use `CancelSynchronousIo` to interrupt a synchronous read or write before join.

## Output completeness

Capture limits and lifecycle interruption are independent. `CommandOutput`
continues to expose `stdout_truncated()` and `stderr_truncated()` for memory
limits, and adds `stdout_complete()` and `stderr_complete()` for whether the
corresponding helper drained its pipe to EOF. Timeout and cancellation errors
may contain partial bytes with `*_complete() == false`; callers must not treat
those bytes as a complete command transcript.
