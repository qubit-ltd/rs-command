// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;
use std::process::ChildStdin;
use std::thread;
use std::thread::JoinHandle;

use process_wrap::std::ChildWrapper;

use super::io_cancellation::IoCancellation;
use super::io_cancellation_token::IoCancellationToken;
use super::pollable_stdin::PollableStdin;
use super::stdin_writer::OptionalStdinWriter;
use super::stdin_writer::StdinWriter;
use crate::CommandError;
use crate::CommandErrorReason;
use crate::session::contracts::SessionBridge;

/// Starts a helper thread that writes configured stdin bytes.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `child` - Spawned child whose stdin pipe may be taken.
/// * `stdin_bytes` - Optional byte buffer to write and then close.
///
/// # Returns
///
/// An optional join handle when buffered stdin is configured.
///
/// # Errors
///
/// Returns a [`CommandError`] with kind `WriteInputFailed` when the configured
/// pipe is missing, or kind `StartInputThreadFailed` when the writer thread
/// cannot be created.
pub(in crate::command_runner) fn write_stdin_bytes(
    command: &str,
    child: &mut dyn ChildWrapper,
    stdin_bytes: Option<Vec<u8>>,
) -> Result<OptionalStdinWriter, CommandError> {
    write_stdin_bytes_with(command, child, stdin_bytes, spawn_stdin_writer)
}

/// Starts the stdin writer for a managed session, draining queued bridge bytes
/// after any configured prefix and closing the child pipe when the session
/// closes.
///
/// # Parameters
///
/// * `command` - Redacted command text used in any startup error.
/// * `child` - Spawned child whose stdin pipe is transferred to a helper.
/// * `stdin_bytes` - Optional prefix written before session bytes.
/// * `bridge` - Session channel used to receive queued stdin bytes and EOF.
///
/// # Returns
///
/// The optional managed writer handle.
///
/// # Errors
///
/// Returns an error if the pipe cannot be prepared or its writer cannot start.
pub(in crate::command_runner) fn write_stdin_session(
    command: &str,
    child: &mut dyn ChildWrapper,
    stdin_bytes: Option<Vec<u8>>,
    bridge: Option<std::sync::Arc<dyn SessionBridge>>,
) -> Result<OptionalStdinWriter, CommandError> {
    if bridge.is_none() {
        return write_stdin_bytes(command, child, stdin_bytes);
    }
    let stdin = child.stdin().take().ok_or_else(|| {
        CommandError::from_reason(
            command,
            CommandErrorReason::WriteInputFailed {
                source: io::Error::other("stdin pipe was not created"),
            },
            None,
        )
    })?;
    prepare_stdin_pipe(&stdin)
        .map_err(|source| CommandError::from_reason(command, CommandErrorReason::WriteInputFailed { source }, None))?;
    let (cancellation, token) = IoCancellation::pair().map_err(|source| {
        CommandError::from_reason(command, CommandErrorReason::StartInputThreadFailed { source }, None)
    })?;
    let bridge = bridge.expect("checked above");
    let bytes = stdin_bytes.unwrap_or_default();
    let writer = thread::Builder::new()
        .name("qubit-command-session-stdin".to_owned())
        .spawn(move || write_session_stdin(stdin, &bytes, bridge, token))
        .map(|join| Some(StdinWriter::new(join, cancellation)));
    map_stdin_thread_result(command, writer)
}

/// Writes a prefix and then drains session bytes until EOF or cancellation.
///
/// # Parameters
///
/// * `stdin` - Child stdin pipe owned by the helper thread.
/// * `prefix` - Initial bytes submitted with the command.
/// * `bridge` - Session bridge that supplies later bytes and EOF.
/// * `token` - Cancellation signal for helper shutdown.
///
/// # Returns
///
/// `Ok(())` after all accepted bytes have been handed to the pipe.
///
/// # Errors
///
/// Returns an I/O error if writing or flushing the pipe fails.
fn write_session_stdin(
    mut stdin: ChildStdin,
    prefix: &[u8],
    bridge: std::sync::Arc<dyn SessionBridge>,
    token: IoCancellationToken,
) -> io::Result<()> {
    if let Err(error) = write_stdin_until_cancelled(&mut stdin, prefix, &token) {
        if error.kind() == io::ErrorKind::BrokenPipe {
            bridge.record_stdin_broken_pipe();
        }
        bridge.close_stdin();
        return Err(error);
    }
    let mut buffer = [0_u8; 8192];
    loop {
        if token.is_cancelled() {
            bridge.close_stdin();
            return Ok(());
        }
        match bridge.read_stdin(&mut buffer) {
            Ok(0) => {
                bridge.close_stdin();
                return Ok(());
            }
            Ok(count) => {
                if let Err(error) = write_stdin_until_cancelled(&mut stdin, &buffer[..count], &token) {
                    if error.kind() == io::ErrorKind::BrokenPipe {
                        bridge.record_stdin_broken_pipe();
                    }
                    bridge.close_stdin();
                    return Err(error);
                }
            }
            Err(error) => {
                bridge.close_stdin();
                return Err(error);
            }
        }
    }
}

/// Takes the child stdin pipe and starts a writer through the supplied spawn
/// operation.
///
/// The injected operation keeps thread creation failures deterministic to test
/// while production callers use [`spawn_stdin_writer`].
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `child` - Spawned child whose stdin pipe may be taken.
/// * `stdin_bytes` - Optional bytes to write; `None` leaves stdin untouched.
/// * `spawn` - Operation that starts the writer thread.
///
/// # Returns
///
/// The optional writer that owns the worker thread and cancellation state.
///
/// # Errors
///
/// Returns `WriteInputFailed` when the child has no stdin pipe or configuring
/// its pipe fails, and `StartInputThreadFailed` when cancellation setup or the
/// supplied spawn operation fails.
fn write_stdin_bytes_with(
    command: &str,
    child: &mut dyn ChildWrapper,
    stdin_bytes: Option<Vec<u8>>,
    spawn: impl FnOnce(ChildStdin, Vec<u8>, IoCancellationToken) -> io::Result<JoinHandle<io::Result<()>>>,
) -> Result<OptionalStdinWriter, CommandError> {
    match stdin_bytes {
        Some(bytes) => match child.stdin().take() {
            Some(stdin) => {
                prepare_stdin_pipe(&stdin).map_err(|source| {
                    CommandError::from_reason(command, CommandErrorReason::WriteInputFailed { source }, None)
                })?;
                let (cancellation, token) = IoCancellation::pair().map_err(|source| {
                    CommandError::from_reason(command, CommandErrorReason::StartInputThreadFailed { source }, None)
                })?;
                let writer = spawn(stdin, bytes, token).map(|join| Some(StdinWriter::new(join, cancellation)));
                map_stdin_thread_result(command, writer)
            }
            None => Err(CommandError::from_reason(
                command,
                CommandErrorReason::WriteInputFailed {
                    source: io::Error::other("stdin pipe was not created"),
                },
                None,
            )),
        },
        None => Ok(None),
    }
}

/// Spawns the production worker that writes bytes to the child's stdin pipe.
///
/// # Parameters
///
/// * `stdin` - Pipe owned by the worker for the duration of the write.
/// * `bytes` - Input bytes written in order until completion or cancellation.
/// * `token` - Cancellation state used to stop blocked or pending writes.
///
/// # Returns
///
/// A join handle that reports the worker's I/O result.
///
/// # Errors
///
/// Returns an I/O error if the operating system cannot create the worker
/// thread.
fn spawn_stdin_writer(
    mut stdin: ChildStdin,
    bytes: Vec<u8>,
    token: IoCancellationToken,
) -> io::Result<JoinHandle<io::Result<()>>> {
    thread::Builder::new()
        .name("qubit-command-stdin-writer".to_owned())
        .spawn(move || write_stdin_until_cancelled(&mut stdin, &bytes, &token))
}

/// Maps stdin worker thread creation to a command error.
///
/// # Parameters
///
/// * `command` - Redacted command text used in the error.
/// * `result` - Thread creation result.
///
/// # Returns
///
/// The created optional writer.
///
/// # Errors
///
/// Returns a [`CommandError`] with kind `StartInputThreadFailed` when the
/// worker cannot be created.
pub(in crate::command_runner) fn map_stdin_thread_result(
    command: &str,
    result: io::Result<OptionalStdinWriter>,
) -> Result<OptionalStdinWriter, CommandError> {
    result.map_err(|source| {
        CommandError::from_reason(command, CommandErrorReason::StartInputThreadFailed { source }, None)
    })
}

/// Joins the stdin writer and maps failures to command errors.
///
/// Broken-pipe errors are accepted because the child may intentionally close
/// stdin before consuming every configured byte.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `writer` - Optional writer-thread join handle.
///
/// # Returns
///
/// `Ok(())` when no writer exists or the writer completes acceptably.
///
/// # Errors
///
/// Returns a [`CommandError`] with kind `WriteInputFailed` for non-broken-pipe
/// write errors or a writer-thread panic.
pub(in crate::command_runner) fn join_stdin_writer(
    command: &str,
    writer: OptionalStdinWriter,
) -> Result<(), CommandError> {
    match writer {
        Some(writer) => match writer.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) if source.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Ok(Err(source)) => Err(CommandError::from_reason(
                command,
                CommandErrorReason::WriteInputFailed { source },
                None,
            )),
            Err(_) => Err(CommandError::from_reason(
                command,
                CommandErrorReason::WriteInputFailed {
                    source: io::Error::other("stdin writer thread panicked"),
                },
                None,
            )),
        },
        None => Ok(()),
    }
}

/// Writes configured stdin bytes until completion or cancellation.
///
/// # Type Parameters
///
/// * `W` - Type used by this operation.
///
/// # Parameters
///
/// * `stdin` - Non-blocking pipe that accepts the bytes.
/// * `bytes` - Input bytes written in order.
/// * `cancellation` - Token checked before waiting and after interrupted
///   writes.
///
/// # Returns
///
/// `Ok(())` when all bytes are written or cancellation stops the operation.
///
/// # Errors
///
/// Returns a pipe I/O error when waiting or writing fails for a reason other
/// than an interrupted or temporarily unavailable operation.
pub(super) fn write_stdin_until_cancelled<W: PollableStdin>(
    stdin: &mut W,
    bytes: &[u8],
    cancellation: &IoCancellationToken,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        if !stdin.wait_writable(cancellation)? {
            return Ok(());
        }
        match stdin.write(&bytes[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "stdin write made no progress"));
            }
            Ok(written) => offset += written,
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => {}
            Err(_source) if cancellation.is_cancelled() => {
                return Ok(());
            }
            Err(source) => return Err(source),
        }
    }
    Ok(())
}

/// Configures one Unix stdin pipe for non-blocking writes.
///
/// # Type Parameters
///
/// * `T` - Pipe handle type exposing the file descriptor to configure.
///
/// # Parameters
///
/// * `pipe` - Pipe handle whose file descriptor flags are updated in place.
///
/// # Returns
///
/// `Ok(())` after non-blocking mode is enabled.
///
/// # Errors
///
/// Returns the operating-system error if reading or updating the descriptor
/// flags fails.
#[cfg(unix)]
fn prepare_stdin_pipe<T: std::os::fd::AsRawFd>(pipe: &T) -> io::Result<()> {
    // SAFETY: fcntl operates on the valid descriptor owned by `pipe`.
    unsafe {
        let flags = libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Leaves Windows stdin handles unchanged because their handles use blocking
/// writes managed by the platform-specific cancellation path.
///
/// # Type Parameters
///
/// * `T` - Child stdin handle type.
///
/// # Parameters
///
/// * `_pipe` - Child stdin handle, which is not modified on Windows.
///
/// # Returns
///
/// Always returns `Ok(())`.
#[cfg(windows)]
fn prepare_stdin_pipe<T>(_pipe: &T) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::cell::Cell;
    #[cfg(unix)]
    use std::collections::VecDeque;
    use std::io;
    #[cfg(unix)]
    use std::io::Write;
    use std::process::Command as ProcessCommand;
    use std::process::Stdio;
    #[cfg(unix)]
    use std::sync::Arc;
    #[cfg(unix)]
    use std::sync::Mutex;
    use std::thread;

    use super::super::io_cancellation::IoCancellation;
    #[cfg(unix)]
    use super::super::io_cancellation_token::IoCancellationToken;
    #[cfg(unix)]
    use super::super::pollable_stdin::PollableStdin;
    use super::super::stdin_writer::StdinWriter;
    use super::join_stdin_writer;
    use super::write_stdin_bytes;
    use super::write_stdin_bytes_with;
    #[cfg(unix)]
    use super::write_stdin_session;
    #[cfg(unix)]
    use super::write_stdin_until_cancelled;
    use crate::CommandErrorKind;

    #[cfg(unix)]
    struct ScriptedStdin {
        wait_ready: bool,
        wait_calls: Cell<usize>,
        write_results: VecDeque<io::Result<usize>>,
        write_calls: usize,
        written: Vec<u8>,
    }

    #[cfg(unix)]
    impl ScriptedStdin {
        fn new(wait_ready: bool, write_results: impl IntoIterator<Item = io::Result<usize>>) -> Self {
            Self {
                wait_ready,
                wait_calls: Cell::new(0),
                write_results: write_results.into_iter().collect(),
                write_calls: 0,
                written: Vec::new(),
            }
        }
    }

    #[cfg(unix)]
    impl Write for ScriptedStdin {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.write_calls += 1;
            match self.write_results.pop_front().unwrap_or(Ok(bytes.len())) {
                Ok(count) => {
                    let count = count.min(bytes.len());
                    self.written.extend_from_slice(&bytes[..count]);
                    Ok(count)
                }
                Err(error) => Err(error),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(unix)]
    impl PollableStdin for ScriptedStdin {
        fn wait_writable(&self, _cancellation: &IoCancellationToken) -> io::Result<bool> {
            self.wait_calls.set(self.wait_calls.get() + 1);
            Ok(self.wait_ready)
        }
    }

    #[cfg(unix)]
    struct TestSessionBridge {
        input: Mutex<VecDeque<Vec<u8>>>,
        fail_read: bool,
        stdin_closed: std::sync::atomic::AtomicBool,
        stdin_broken_pipe: std::sync::atomic::AtomicBool,
    }

    #[cfg(unix)]
    impl TestSessionBridge {
        fn new(input: impl IntoIterator<Item = Vec<u8>>, fail_read: bool) -> Self {
            Self {
                input: Mutex::new(input.into_iter().collect()),
                fail_read,
                stdin_closed: std::sync::atomic::AtomicBool::new(false),
                stdin_broken_pipe: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[cfg(unix)]
    impl crate::session::contracts::SessionBridge for TestSessionBridge {
        fn read_stdin(&self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.fail_read {
                return Err(io::Error::other("injected session stdin read failure"));
            }
            let Some(bytes) = self.input.lock().unwrap().pop_front() else {
                return Ok(0);
            };
            buffer[..bytes.len()].copy_from_slice(&bytes);
            Ok(bytes.len())
        }

        fn write_stdout(&self, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }

        fn write_stderr(&self, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }

        fn close_stdin(&self) {
            self.stdin_closed.store(true, std::sync::atomic::Ordering::SeqCst);
        }

        fn record_stdin_broken_pipe(&self) {
            self.stdin_broken_pipe.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn test_write_stdin_bytes_reports_missing_pipe() {
        let mut child = ProcessCommand::new("rustc")
            .arg("--version")
            .stdin(Stdio::null())
            .spawn()
            .expect("test child should spawn");

        let error = write_stdin_bytes("command", &mut child, Some(b"input".to_vec()))
            .expect_err("missing stdin pipe should be reported");

        assert_eq!(error.kind(), CommandErrorKind::WriteInputFailed);
        child.wait().expect("test child should be waitable");
    }

    #[test]
    fn test_write_stdin_bytes_delivers_payload_and_closes_child_stdin() {
        let mut child = ProcessCommand::new("sh")
            .arg("-c")
            .arg("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("test child should spawn");

        let writer = write_stdin_bytes("command", &mut child, Some(b"writer payload".to_vec()))
            .expect("stdin writer should start");
        join_stdin_writer("command", writer).expect("stdin writer should finish");
        let output = child
            .wait_with_output()
            .expect("test child should finish after stdin closes");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"writer payload");
    }

    #[test]
    fn test_write_stdin_bytes_without_payload_starts_no_writer() {
        let mut child = ProcessCommand::new("sh")
            .arg("-c")
            .arg("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("test child should spawn");

        let writer =
            write_stdin_bytes("command", &mut child, None).expect("absent stdin bytes should not require a worker");
        join_stdin_writer("command", writer).expect("an absent worker should join successfully");
        drop(child.stdin.take());
        let output = child
            .wait_with_output()
            .expect("test child should finish after stdin closes");

        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn test_join_stdin_writer_accepts_child_closing_stdin_early() {
        let mut child = ProcessCommand::new("sh")
            .arg("-c")
            .arg("exit 0")
            .stdin(Stdio::piped())
            .spawn()
            .expect("test child should spawn");

        let writer =
            write_stdin_bytes("command", &mut child, Some(vec![b'x'; 1024 * 1024])).expect("stdin writer should start");
        join_stdin_writer("command", writer).expect("early child stdin closure should be accepted as a broken pipe");
        let status = child.wait().expect("test child should be waitable");

        assert!(status.success());
    }

    #[test]
    fn test_write_stdin_bytes_maps_injected_spawn_failure() {
        let mut child = ProcessCommand::new("rustc")
            .arg("--version")
            .stdin(Stdio::piped())
            .spawn()
            .expect("test child should spawn");

        let error = write_stdin_bytes_with(
            "command",
            &mut child,
            Some(b"input".to_vec()),
            |_stdin, _bytes, _token| Err(io::Error::other("injected spawn failure")),
        )
        .expect_err("injected stdin worker failure should be mapped");

        assert_eq!(error.kind(), CommandErrorKind::StartInputThreadFailed);
        child.wait().expect("test child should be waitable");
    }

    #[test]
    fn test_join_stdin_writer_maps_write_failure() {
        let (cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        let writer = StdinWriter::new(
            thread::spawn(move || {
                let _token = token;
                Err(io::Error::other("injected stdin write failure"))
            }),
            cancellation,
        );

        let error = join_stdin_writer("command", Some(writer)).expect_err("stdin write failure should be mapped");

        assert_eq!(error.kind(), CommandErrorKind::WriteInputFailed);
    }

    #[test]
    fn test_join_stdin_writer_maps_worker_panic() {
        let (cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        let writer = StdinWriter::new(
            thread::spawn(move || -> io::Result<()> {
                let _token = token;
                panic!("injected stdin worker panic");
            }),
            cancellation,
        );

        let error = join_stdin_writer("command", Some(writer)).expect_err("stdin worker panic should be mapped");

        assert_eq!(error.kind(), CommandErrorKind::WriteInputFailed);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_stdin_until_cancelled_retries_transient_errors_and_partial_writes() {
        let (_cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        let mut stdin = ScriptedStdin::new(
            true,
            [
                Err(io::ErrorKind::Interrupted.into()),
                Err(io::ErrorKind::WouldBlock.into()),
                Ok(2),
                Ok(3),
            ],
        );

        write_stdin_until_cancelled(&mut stdin, b"hello", &token).expect("transient writes should retry");

        assert_eq!(stdin.written, b"hello");
        assert_eq!(stdin.write_calls, 4);
        assert_eq!(stdin.wait_calls.get(), 4);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_stdin_until_cancelled_stops_and_preserves_write_errors() {
        let (_cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        token.cancelled.store(true, std::sync::atomic::Ordering::Release);
        let mut cancelled_stdin = ScriptedStdin::new(true, []);

        write_stdin_until_cancelled(&mut cancelled_stdin, b"not written", &token)
            .expect("cancellation should stop the write successfully");
        assert_eq!(cancelled_stdin.write_calls, 0);
        assert_eq!(cancelled_stdin.wait_calls.get(), 0);

        let (_cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        let mut zero_writer = ScriptedStdin::new(true, [Ok(0)]);
        let error = write_stdin_until_cancelled(&mut zero_writer, b"data", &token)
            .expect_err("a zero-length write must be reported");
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);

        let mut broken_writer = ScriptedStdin::new(true, [Err(io::ErrorKind::BrokenPipe.into())]);
        let error = write_stdin_until_cancelled(&mut broken_writer, b"data", &token)
            .expect_err("non-cancellation write errors must be propagated");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_stdin_until_cancelled_stops_when_pipe_is_not_ready() {
        let (_cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        let mut stdin = ScriptedStdin::new(false, []);

        write_stdin_until_cancelled(&mut stdin, b"deferred", &token)
            .expect("an unwritable pipe should stop without failing");

        assert_eq!(stdin.wait_calls.get(), 1);
        assert_eq!(stdin.write_calls, 0);
        assert!(stdin.written.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_stdin_until_cancelled_does_not_wait_for_empty_input() {
        let (_cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        let mut stdin = ScriptedStdin::new(false, []);

        write_stdin_until_cancelled(&mut stdin, b"", &token).expect("empty input needs no pipe readiness");

        assert_eq!(stdin.wait_calls.get(), 0);
        assert_eq!(stdin.write_calls, 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_session_stdin_writes_prefix_and_queued_bytes_before_closing() {
        let bridge = Arc::new(TestSessionBridge::new([b"session".to_vec()], false));
        let mut child = ProcessCommand::new("sh")
            .arg("-c")
            .arg("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("test child should spawn");

        let writer = write_stdin_session("command", &mut child, Some(b"prefix".to_vec()), Some(bridge.clone()))
            .expect("session stdin writer should start");
        join_stdin_writer("command", writer).expect("session stdin should finish");
        let output = child.wait_with_output().expect("test child should finish");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"prefixsession");
        assert!(bridge.stdin_closed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn test_session_stdin_read_failure_closes_pipe_and_reaches_joiner() {
        let bridge = Arc::new(TestSessionBridge::new([], true));
        let mut child = ProcessCommand::new("sh")
            .arg("-c")
            .arg("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("test child should spawn");

        let writer = write_stdin_session("command", &mut child, None, Some(bridge.clone()))
            .expect("session stdin writer should start");
        let error = join_stdin_writer("command", writer).expect_err("session read failure should be reported");
        let output = child
            .wait_with_output()
            .expect("test child should finish after stdin closes");

        assert_eq!(error.kind(), CommandErrorKind::WriteInputFailed);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(bridge.stdin_closed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn test_session_stdin_records_child_broken_pipe_and_closes_bridge() {
        let bridge = Arc::new(TestSessionBridge::new([], false));
        let mut child = ProcessCommand::new("sh")
            .arg("-c")
            .arg("exit 0")
            .stdin(Stdio::piped())
            .spawn()
            .expect("test child should spawn");

        let writer = write_stdin_session(
            "command",
            &mut child,
            Some(vec![b'x'; 4 * 1024 * 1024]),
            Some(bridge.clone()),
        )
        .expect("session stdin writer should start");
        join_stdin_writer("command", writer).expect("an early child stdin close is accepted");
        let status = child.wait().expect("test child should finish");

        assert!(status.success());
        assert!(bridge.stdin_broken_pipe.load(std::sync::atomic::Ordering::SeqCst));
        assert!(bridge.stdin_closed.load(std::sync::atomic::Ordering::SeqCst));
    }
}
