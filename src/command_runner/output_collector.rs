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
        Read,
        Write,
    },
    process::ExitStatus,
    thread,
    time::Duration,
};

use super::{
    captured_output::CapturedOutput,
    output_capture_error::OutputCaptureError,
    output_capture_options::OutputCaptureOptions,
    output_reader::OutputReader,
    stdin_pipe::join_stdin_writer,
    stdin_writer::StdinWriter,
};
use crate::{
    CommandError,
    CommandOutput,
    OutputStream,
};

/// Spawns a reader thread for a child output stream.
pub(crate) fn read_output_stream(
    mut reader: Box<dyn Read + Send>,
    options: OutputCaptureOptions,
) -> OutputReader {
    thread::spawn(move || read_output(reader.as_mut(), options))
}

/// Reads one child output stream to completion.
pub(crate) fn read_output(
    reader: &mut dyn Read,
    mut options: OutputCaptureOptions,
) -> Result<CapturedOutput, OutputCaptureError> {
    let mut bytes = Vec::new();
    if let Some(max_bytes) = options.max_bytes {
        bytes.reserve(max_bytes.min(8 * 1024));
    }
    let mut truncated = false;
    let mut write_error = None;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(OutputCaptureError::Read)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if let Some(tee) = options.tee.as_mut()
            && write_error.is_none()
            && let Err(source) = tee.writer.write_all(chunk)
        {
            write_error = Some(OutputCaptureError::Write {
                path: tee.path.clone(),
                source,
            });
            options.tee = None;
        }
        match options.max_bytes {
            Some(max_bytes) => {
                let remaining = max_bytes.saturating_sub(bytes.len());
                if remaining > 0 {
                    let retained = remaining.min(chunk.len());
                    bytes.extend_from_slice(&chunk[..retained]);
                }
                if chunk.len() > remaining {
                    truncated = true;
                }
            }
            None => bytes.extend_from_slice(chunk),
        }
    }
    if write_error.is_none()
        && let Some(tee) = options.tee.as_mut()
        && let Err(source) = tee.writer.flush()
    {
        write_error = Some(OutputCaptureError::Write {
            path: tee.path.clone(),
            source,
        });
    }
    if let Some(error) = write_error {
        Err(error)
    } else {
        Ok(CapturedOutput { bytes, truncated })
    }
}

/// Collects reader-thread results into a command output value.
pub(crate) fn collect_output(
    command: &str,
    status: ExitStatus,
    elapsed: Duration,
    lossy_output: bool,
    stdout_reader: OutputReader,
    stderr_reader: OutputReader,
    stdin_writer: StdinWriter,
) -> Result<CommandOutput, CommandError> {
    #[cfg(coverage)]
    crate::coverage_support::record_collect_output(command);

    #[cfg(coverage)]
    let forced_error =
        crate::coverage_support::forced_collect_output_error(command).map(|stream| {
            CommandError::ReadOutputFailed {
                command: command.to_owned(),
                stream,
                source: io::Error::other("forced output collection failure"),
            }
        });
    #[cfg(not(coverage))]
    let forced_error = None;

    let stdout_result = join_output_reader(command, OutputStream::Stdout, stdout_reader);
    let stderr_result = join_output_reader(command, OutputStream::Stderr, stderr_reader);
    let stdin_result = join_stdin_writer(command, stdin_writer);

    match (stdout_result, stderr_result, stdin_result, forced_error) {
        (Ok(stdout), Ok(stderr), Ok(()), None) => Ok(CommandOutput::new(
            status,
            stdout.bytes,
            stderr.bytes,
            stdout.truncated,
            stderr.truncated,
            elapsed,
            lossy_output,
        )),
        (Err(error), _, _, _)
        | (_, Err(error), _, _)
        | (_, _, Err(error), _)
        | (_, _, _, Some(error)) => Err(error),
    }
}

/// Joins one output reader and maps failures to command errors.
pub(crate) fn join_output_reader(
    command: &str,
    stream: OutputStream,
    reader: OutputReader,
) -> Result<CapturedOutput, CommandError> {
    match reader.join() {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(OutputCaptureError::Read(source))) => Err(CommandError::ReadOutputFailed {
            command: command.to_owned(),
            stream,
            source,
        }),
        Ok(Err(OutputCaptureError::Write { path, source })) => {
            Err(CommandError::WriteOutputFailed {
                command: command.to_owned(),
                stream,
                path,
                source,
            })
        }
        Err(_) => Err(CommandError::ReadOutputFailed {
            command: command.to_owned(),
            stream,
            source: io::Error::other("output reader thread panicked"),
        }),
    }
}
