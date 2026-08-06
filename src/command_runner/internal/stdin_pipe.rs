// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
#[cfg(coverage)]
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::{
    io::{
        self,
        Write,
    },
    thread,
};

use process_wrap::std::ChildWrapper;

use super::stdin_writer::StdinWriter;
use crate::CommandError;

#[cfg(coverage)]
static COVERAGE_FAIL_STDIN_THREAD: AtomicBool = AtomicBool::new(false);

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
) -> Result<StdinWriter, CommandError> {
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
                let writer = thread::Builder::new()
                    .name("qubit-command-stdin-writer".to_owned())
                    .spawn(move || stdin.write_all(&bytes))
                    .map(Some);
                map_stdin_thread_result(command, writer)
            }
            None => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source: io::Error::other("stdin pipe was not created"),
            }),
        },
        None => Ok(None),
    }
}

pub(in crate::command_runner) fn map_stdin_thread_result(
    command: &str,
    result: io::Result<StdinWriter>,
) -> Result<StdinWriter, CommandError> {
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
    writer: StdinWriter,
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
            }),
            Err(_) => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source: io::Error::other("stdin writer thread panicked"),
            }),
        },
        None => Ok(()),
    }
}
