# Command Runner I/O Lifecycle Design

## Purpose

`qubit-command` treats command I/O as part of the command lifecycle. Timeout and
cancellation cleanup must not detach library-owned helper threads, and returned
output must state whether each stream was completely drained.

## Ordinary-file boundary

`Command::stdin_file`, `CommandRunOptions::tee_stdout_to_file`, and
`CommandRunOptions::tee_stderr_to_file` accept ordinary files only. Existing paths
are checked through their metadata; a missing tee path may be created as a
regular file. FIFOs, devices, sockets, directories, and other special files are
rejected before the child is spawned. A symlink is accepted only when its target
is an ordinary file.

This boundary prevents a FIFO or device from blocking the command's I/O helper
forever. It does not promise that a broken remote or FUSE filesystem can never
stall an operating-system file operation; such external filesystem failures are
outside the command timeout contract.

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
