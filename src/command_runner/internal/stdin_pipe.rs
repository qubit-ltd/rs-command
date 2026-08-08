// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
use std::io;
#[cfg(coverage)]
use std::sync::atomic::AtomicBool;
#[cfg(coverage)]
use std::sync::atomic::Ordering;
use std::thread;

use process_wrap::std::ChildWrapper;

use super::io_cancellation::IoCancellation;
use super::io_cancellation_token::IoCancellationToken;
use super::pollable_stdin::PollableStdin;
use super::stdin_writer::OptionalStdinWriter;
use super::stdin_writer::StdinWriter;
use crate::CommandError;

#[cfg(coverage)]
static COVERAGE_FAIL_STDIN_THREAD: AtomicBool = AtomicBool::new(false);

/// Enables or disables deterministic stdin-thread failure injection.
#[cfg(coverage)]
pub(in crate::command_runner) fn __coverage_fail_stdin_thread(enabled: bool) {
    COVERAGE_FAIL_STDIN_THREAD.store(enabled, Ordering::Relaxed);
}

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
/// Returns [`CommandError::WriteInputFailed`] when the configured pipe is
/// missing, or [`CommandError::StartInputThreadFailed`] when the writer thread
/// cannot be created.
pub(in crate::command_runner) fn write_stdin_bytes(
    command: &str,
    child: &mut dyn ChildWrapper,
    stdin_bytes: Option<Vec<u8>>,
) -> Result<OptionalStdinWriter, CommandError> {
    match stdin_bytes {
        Some(bytes) => match child.stdin().take() {
            Some(mut stdin) => {
                #[cfg(coverage)]
                if COVERAGE_FAIL_STDIN_THREAD.load(Ordering::Relaxed) {
                    return Err(CommandError::StartInputThreadFailed {
                        command: command.to_owned(),
                        source: io::Error::other(
                            "coverage-injected stdin thread failure",
                        ),
                    });
                }
                prepare_stdin_pipe(&stdin).map_err(|source| {
                    CommandError::WriteInputFailed {
                        command: command.to_owned(),
                        source,
                        output: None,
                    }
                })?;
                let (cancellation, token) =
                    IoCancellation::pair().map_err(|source| {
                        CommandError::StartInputThreadFailed {
                            command: command.to_owned(),
                            source,
                        }
                    })?;
                let writer = thread::Builder::new()
                    .name("qubit-command-stdin-writer".to_owned())
                    .spawn(move || {
                        write_stdin_until_cancelled(&mut stdin, &bytes, token)
                    })
                    .map(|join| Some(StdinWriter::new(join, cancellation)));
                map_stdin_thread_result(command, writer)
            }
            None => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source: io::Error::other("stdin pipe was not created"),
                output: None,
            }),
        },
        None => Ok(None),
    }
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
/// Returns [`CommandError::StartInputThreadFailed`] when the worker cannot be
/// created.
pub(in crate::command_runner) fn map_stdin_thread_result(
    command: &str,
    result: io::Result<OptionalStdinWriter>,
) -> Result<OptionalStdinWriter, CommandError> {
    result.map_err(|source| CommandError::StartInputThreadFailed {
        command: command.to_owned(),
        source,
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
/// Returns [`CommandError::WriteInputFailed`] for non-broken-pipe write errors
/// or a writer-thread panic.
pub(in crate::command_runner) fn join_stdin_writer(
    command: &str,
    writer: OptionalStdinWriter,
) -> Result<(), CommandError> {
    match writer {
        Some(writer) => match writer.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) if source.kind() == io::ErrorKind::BrokenPipe => {
                Ok(())
            }
            Ok(Err(source)) => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source,
                output: None,
            }),
            Err(_) => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source: io::Error::other("stdin writer thread panicked"),
                output: None,
            }),
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
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "stdin write made no progress",
                ));
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
        if libc::fcntl(
            pipe.as_raw_fd(),
            libc::F_SETFL,
            flags | libc::O_NONBLOCK,
        ) < 0
        {
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
