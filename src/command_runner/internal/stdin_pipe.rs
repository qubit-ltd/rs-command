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

/// Starts a stdin helper with the supplied thread-spawn operation.
fn write_stdin_bytes_with(
    command: &str,
    child: &mut dyn ChildWrapper,
    stdin_bytes: Option<Vec<u8>>,
    spawn: impl FnOnce(
        ChildStdin,
        Vec<u8>,
        IoCancellationToken,
    ) -> io::Result<JoinHandle<io::Result<()>>>,
) -> Result<OptionalStdinWriter, CommandError> {
    match stdin_bytes {
        Some(bytes) => match child.stdin().take() {
            Some(stdin) => {
                prepare_stdin_pipe(&stdin).map_err(|source| {
                    CommandError::from_reason(
                        command,
                        CommandErrorReason::WriteInputFailed { source },
                        None,
                    )
                })?;
                let (cancellation, token) = IoCancellation::pair().map_err(|source| {
                    CommandError::from_reason(
                        command,
                        CommandErrorReason::StartInputThreadFailed { source },
                        None,
                    )
                })?;
                let writer = spawn(stdin, bytes, token)
                    .map(|join| Some(StdinWriter::new(join, cancellation)));
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

/// Spawns the production stdin writer thread.
fn spawn_stdin_writer(
    mut stdin: ChildStdin,
    bytes: Vec<u8>,
    token: IoCancellationToken,
) -> io::Result<JoinHandle<io::Result<()>>> {
    thread::Builder::new()
        .name("qubit-command-stdin-writer".to_owned())
        .spawn(move || write_stdin_until_cancelled(&mut stdin, &bytes, token))
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
fn write_stdin_until_cancelled<W: PollableStdin>(
    stdin: &mut W,
    bytes: &[u8],
    cancellation: IoCancellationToken,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        if !stdin.wait_writable(&cancellation)? {
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

/// Leaves Windows stdin handles unchanged.
#[cfg(windows)]
fn prepare_stdin_pipe<T>(_pipe: &T) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::process::Command as ProcessCommand;
    use std::process::Stdio;
    use std::thread;

    use super::super::io_cancellation::IoCancellation;
    use super::super::stdin_writer::StdinWriter;
    use super::join_stdin_writer;
    use super::write_stdin_bytes;
    use super::write_stdin_bytes_with;
    use crate::CommandErrorKind;

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
        let (cancellation, token) =
            IoCancellation::pair().expect("cancellation pair should be created");
        let writer = StdinWriter::new(
            thread::spawn(move || {
                let _token = token;
                Err(io::Error::other("injected stdin write failure"))
            }),
            cancellation,
        );

        let error = join_stdin_writer("command", Some(writer))
            .expect_err("stdin write failure should be mapped");

        assert_eq!(error.kind(), CommandErrorKind::WriteInputFailed);
    }

    #[test]
    fn test_join_stdin_writer_maps_worker_panic() {
        let (cancellation, token) =
            IoCancellation::pair().expect("cancellation pair should be created");
        let writer = StdinWriter::new(
            thread::spawn(move || -> io::Result<()> {
                let _token = token;
                panic!("injected stdin worker panic");
            }),
            cancellation,
        );

        let error = join_stdin_writer("command", Some(writer))
            .expect_err("stdin worker panic should be mapped");

        assert_eq!(error.kind(), CommandErrorKind::WriteInputFailed);
    }
}
