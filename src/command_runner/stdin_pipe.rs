/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
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

/// Starts a helper thread that writes configured stdin bytes.
pub(crate) fn write_stdin_bytes(
    command: &str,
    child: &mut dyn ChildWrapper,
    stdin_bytes: Option<Vec<u8>>,
) -> Result<StdinWriter, CommandError> {
    match stdin_bytes {
        Some(bytes) => match child.stdin().take() {
            Some(mut stdin) => Ok(Some(thread::spawn(move || stdin.write_all(&bytes)))),
            None => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source: io::Error::other("stdin pipe was not created"),
            }),
        },
        None => Ok(None),
    }
}

/// Joins the stdin writer and maps failures to command errors.
pub(crate) fn join_stdin_writer(command: &str, writer: StdinWriter) -> Result<(), CommandError> {
    match writer {
        Some(writer) => match writer.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) if source.kind() == io::ErrorKind::BrokenPipe => Ok(()),
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
