// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
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

use qubit_clock::TimeError;

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
///
/// # Parameters
///
/// * `reader` - Child output pipe to drain.
/// * `options` - In-memory capture limit and optional tee destination.
///
/// # Returns
///
/// Join handle for the named output-reader thread.
///
/// # Errors
///
/// Returns the thread builder's I/O error when the helper cannot be started.
#[inline]
pub(in crate::command_runner) fn read_output_stream(
    mut reader: Box<dyn Read + Send>,
    options: OutputCaptureOptions,
) -> io::Result<OutputReader> {
    thread::Builder::new()
        .name("qubit-command-output-reader".to_owned())
        .spawn(move || read_output(reader.as_mut(), options))
}

/// Reads one child output stream to completion.
///
/// # Parameters
///
/// * `reader` - Child output pipe to drain.
/// * `options` - In-memory capture limit and optional tee destination.
///
/// # Returns
///
/// Retained bytes and their truncation state.
///
/// # Errors
///
/// Returns [`OutputCaptureError::Read`] for pipe reads or
/// [`OutputCaptureError::Write`] for tee writes and flushing. The reader still
/// drains the child pipe after the first tee failure.
pub(in crate::command_runner) fn read_output(
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
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(source) => return Err(OutputCaptureError::Read(source)),
        };
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if let Some(tee) = options.tee.as_mut()
            && write_error.is_none()
            && let Err(source) = tee.writer.write_all(chunk)
        {
            write_error = Some((tee.path.clone(), source));
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
        write_error = Some((tee.path.clone(), source));
    }
    if let Some((path, source)) = write_error {
        Err(OutputCaptureError::Write {
            path,
            source,
            output: CapturedOutput { bytes, truncated },
        })
    } else {
        Ok(CapturedOutput { bytes, truncated })
    }
}

/// Collects reader-thread results into a command output value.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `status` - Child exit status.
/// * `elapsed` - Callback that samples command duration after every helper has
///   been joined.
/// * `stdout_reader` - Helper draining stdout.
/// * `stderr_reader` - Helper draining stderr.
/// * `stdin_writer` - Optional helper writing stdin.
///
/// # Returns
///
/// Captured command output after every helper has been joined.
///
/// # Errors
///
/// Returns a time-handling failure after joining every helper, otherwise the
/// first stdout, stderr, or stdin helper failure in that order.
pub(in crate::command_runner) fn collect_output<F>(
    command: &str,
    status: ExitStatus,
    elapsed: F,
    stdout_reader: OutputReader,
    stderr_reader: OutputReader,
    stdin_writer: StdinWriter,
) -> Result<CommandOutput, CommandError>
where
    F: FnOnce() -> Result<Duration, TimeError>,
{
    let stdout_result = join_output_reader(stdout_reader);
    let stderr_result = join_output_reader(stderr_reader);
    let stdin_result = join_stdin_writer(command, stdin_writer);
    let elapsed_result = elapsed();

    match (elapsed_result, stdout_result, stderr_result, stdin_result) {
        (Err(source), _, _, _) => Err(CommandError::TimeFailed {
            command: command.to_owned(),
            source,
        }),
        (Ok(elapsed), Ok(stdout), Ok(stderr), Ok(())) => {
            Ok(CommandOutput::new(
                status,
                stdout.bytes,
                stderr.bytes,
                stdout.truncated,
                stderr.truncated,
                elapsed,
            ))
        }
        (Ok(elapsed), Err(error), _, _) => Err(map_output_reader_error(
            command,
            status,
            elapsed,
            OutputStream::Stdout,
            error,
            None,
        )),
        (Ok(elapsed), Ok(stdout), Err(error), _) => {
            Err(map_output_reader_error(
                command,
                status,
                elapsed,
                OutputStream::Stderr,
                error,
                Some(stdout),
            ))
        }
        (Ok(_), _, _, Err(error)) => Err(error),
    }
}

/// Maps a reader result while retaining output from a failed tee write.
fn map_output_reader_error(
    command: &str,
    status: ExitStatus,
    elapsed: Duration,
    stream: OutputStream,
    error: OutputCaptureError,
    other_output: Option<CapturedOutput>,
) -> CommandError {
    match error {
        OutputCaptureError::Read(source) => CommandError::ReadOutputFailed {
            command: command.to_owned(),
            stream,
            source,
        },
        OutputCaptureError::Write {
            path,
            source,
            output,
        } => {
            let (stdout, stderr) = match stream {
                OutputStream::Stdout => {
                    (output, other_output.unwrap_or_default())
                }
                OutputStream::Stderr => {
                    (other_output.unwrap_or_default(), output)
                }
            };
            CommandError::WriteOutputFailed {
                command: command.to_owned(),
                stream,
                path,
                source,
                output: Some(Box::new(CommandOutput::new(
                    status,
                    stdout.bytes,
                    stderr.bytes,
                    stdout.truncated,
                    stderr.truncated,
                    elapsed,
                ))),
            }
        }
    }
}

/// Joins one output reader and maps failures to command errors.
///
/// # Parameters
///
/// * `reader` - Reader-thread join handle.
///
/// # Returns
///
/// Captured bytes and truncation state from the reader.
///
/// # Errors
///
/// Returns [`CommandError::ReadOutputFailed`] for read failures or thread
/// panics, and [`CommandError::WriteOutputFailed`] for tee failures.
pub(in crate::command_runner) fn join_output_reader(
    reader: OutputReader,
) -> Result<CapturedOutput, OutputCaptureError> {
    match reader.join() {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(OutputCaptureError::Read(io::Error::other(
            "output reader thread panicked",
        ))),
    }
}
