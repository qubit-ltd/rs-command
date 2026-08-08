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
    cancellable_reader::CancellableReader,
    captured_output::CapturedOutput,
    io_cancellation::IoCancellation,
    io_cancellation_token::IoCancellationToken,
    output_capture_error::OutputCaptureError,
    output_capture_options::OutputCaptureOptions,
    output_reader::OutputReader,
    stdin_pipe::join_stdin_writer,
    stdin_writer::OptionalStdinWriter,
};
use crate::{
    CommandError,
    CommandOutput,
    OutputStream,
};

#[cfg(unix)]
type OutputFd = std::os::fd::RawFd;
#[cfg(windows)]
type OutputFd = ();

/// Starts a cancellation-aware helper thread for one output stream.
///
/// # Parameters
///
/// * `reader` - Child stdout or stderr pipe.
/// * `options` - In-memory limit and optional tee destination.
///
/// # Returns
///
/// An output reader owning the helper thread.
///
/// # Errors
///
/// Returns an I/O error when the pipe cannot be prepared, cancellation state
/// cannot be created, or the helper thread cannot be spawned.
#[inline]
pub(in crate::command_runner) fn read_output_stream<R: CancellableReader>(
    reader: R,
    options: OutputCaptureOptions,
) -> io::Result<OutputReader> {
    reader.prepare_for_cancellation()?;
    let (cancellation, token) = IoCancellation::pair()?;
    let join = thread::Builder::new()
        .name("qubit-command-output-reader".to_owned())
        .spawn(move || read_output_until_cancelled(reader, options, token))?;
    Ok(OutputReader::new(join, cancellation))
}

/// Reads one output stream until EOF or cancellation is requested.
fn read_output_until_cancelled<R: CancellableReader>(
    mut reader: R,
    options: OutputCaptureOptions,
    cancellation: IoCancellationToken,
) -> Result<CapturedOutput, OutputCaptureError> {
    #[cfg(unix)]
    let fd = Some(reader.raw_fd());
    #[cfg(windows)]
    let fd = None;
    read_output_inner(&mut reader, options, Some(&cancellation), fd)
}

/// Drains one output stream while retaining bounded bytes and teeing the full
/// stream when configured.
///
/// # Parameters
///
/// * `reader` - Output stream reader.
/// * `options` - Capture limit and optional tee writer.
/// * `cancellation` - Optional cancellation token.
/// * `fd` - Unix descriptor used for event-driven polling.
///
/// # Returns
///
/// Captured bytes and stream completion metadata.
///
/// # Errors
///
/// Returns an output read or tee write error.
fn read_output_inner(
    reader: &mut dyn Read,
    mut options: OutputCaptureOptions,
    cancellation: Option<&IoCancellationToken>,
    fd: Option<OutputFd>,
) -> Result<CapturedOutput, OutputCaptureError> {
    #[cfg(not(unix))]
    let _ = fd;
    let mut bytes = Vec::new();
    if let Some(max_bytes) = options.max_bytes {
        bytes.reserve(max_bytes.min(8 * 1024));
    }
    let mut truncated = false;
    let mut write_error = None;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        if cancellation.is_some_and(IoCancellationToken::is_cancelled) {
            return Ok(CapturedOutput {
                bytes,
                truncated,
                complete: false,
            });
        }
        #[cfg(unix)]
        if let (Some(cancellation), Some(fd)) = (cancellation, fd) {
            let ready = cancellation.wait_for_fd(fd, libc::POLLIN).map_err(
                |source| read_capture_error(source, bytes.clone(), truncated),
            )?;
            if !ready {
                return Ok(CapturedOutput {
                    bytes,
                    truncated,
                    complete: false,
                });
            }
        }
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                if cancellation.is_some_and(IoCancellationToken::is_cancelled) {
                    return Ok(CapturedOutput {
                        bytes,
                        truncated,
                        complete: false,
                    });
                }
                if cancellation.is_none() {
                    thread::sleep(Duration::from_millis(1));
                }
                continue;
            }
            Err(_source)
                if cancellation
                    .is_some_and(IoCancellationToken::is_cancelled) =>
            {
                return Ok(CapturedOutput {
                    bytes,
                    truncated,
                    complete: false,
                });
            }
            Err(source) => {
                return Err(read_capture_error(source, bytes, truncated));
            }
        };
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if let Some(tee) = options.tee.as_mut()
            && write_error.is_none()
            && let Err(source) = tee.writer.write_all(chunk)
        {
            if cancellation.is_some_and(IoCancellationToken::is_cancelled) {
                return Ok(CapturedOutput {
                    bytes,
                    truncated,
                    complete: false,
                });
            }
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
        if cancellation.is_some_and(IoCancellationToken::is_cancelled) {
            return Ok(CapturedOutput {
                bytes,
                truncated,
                complete: false,
            });
        }
        write_error = Some((tee.path.clone(), source));
    }
    if let Some((path, source)) = write_error {
        Err(OutputCaptureError::Write {
            path,
            source,
            output: CapturedOutput {
                bytes,
                truncated,
                complete: true,
            },
        })
    } else {
        Ok(CapturedOutput {
            bytes,
            truncated,
            complete: true,
        })
    }
}

/// Builds a read error with the bytes retained before the failure.
fn read_capture_error(
    source: io::Error,
    bytes: Vec<u8>,
    truncated: bool,
) -> OutputCaptureError {
    OutputCaptureError::Read {
        source,
        output: CapturedOutput {
            bytes,
            truncated,
            complete: false,
        },
    }
}

/// Reads one child output stream to completion for tests and coverage hooks.
#[allow(dead_code)]
pub(in crate::command_runner) fn read_output(
    reader: &mut dyn Read,
    options: OutputCaptureOptions,
) -> Result<CapturedOutput, OutputCaptureError> {
    read_output_inner(reader, options, None, None)
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
    stdin_writer: OptionalStdinWriter,
) -> Result<CommandOutput, CommandError>
where
    F: FnOnce() -> Result<Duration, TimeError>,
{
    let stdout_result = join_output_reader(stdout_reader);
    let stderr_result = join_output_reader(stderr_reader);
    let stdin_result = join_stdin_writer(command, stdin_writer);
    let elapsed_result = elapsed();

    collect_output_results(
        command,
        status,
        elapsed_result,
        stdout_result,
        stderr_result,
        stdin_result,
    )
}

/// Builds command output from completed helper results.
///
/// # Parameters
///
/// * `command` - Redacted command text used in errors.
/// * `status` - Child exit status.
/// * `elapsed_result` - Sampled command duration.
/// * `stdout_result` - Completed stdout helper result.
/// * `stderr_result` - Completed stderr helper result.
/// * `stdin_result` - Completed stdin helper result.
///
/// # Returns
///
/// Captured command output after mapping helper failures.
///
/// # Errors
///
/// Returns a time-handling failure, otherwise the first stdout, stderr, or
/// stdin helper failure in that order.
pub(in crate::command_runner) fn collect_output_results(
    command: &str,
    status: ExitStatus,
    elapsed_result: Result<Duration, TimeError>,
    stdout_result: Result<CapturedOutput, OutputCaptureError>,
    stderr_result: Result<CapturedOutput, OutputCaptureError>,
    stdin_result: Result<(), CommandError>,
) -> Result<CommandOutput, CommandError> {
    let elapsed = match elapsed_result {
        Err(source) => {
            return Err(CommandError::TimeFailed {
                command: command.to_owned(),
                source,
            });
        }
        Ok(elapsed) => elapsed,
    };
    let stdout = match stdout_result {
        Err(error) => {
            return Err(map_output_reader_error(
                command,
                status,
                elapsed,
                OutputStream::Stdout,
                error,
                Some(retained_output(stderr_result)),
            ));
        }
        Ok(stdout) => stdout,
    };
    let stderr = match stderr_result {
        Err(error) => {
            return Err(map_output_reader_error(
                command,
                status,
                elapsed,
                OutputStream::Stderr,
                error,
                Some(stdout),
            ));
        }
        Ok(stderr) => stderr,
    };
    let output = CommandOutput::new(
        status,
        (stdout.bytes, stdout.truncated, stdout.complete),
        (stderr.bytes, stderr.truncated, stderr.complete),
        elapsed,
    );
    match stdin_result {
        Ok(()) => Ok(output),
        Err(CommandError::WriteInputFailed {
            command, source, ..
        }) => Err(CommandError::WriteInputFailed {
            command,
            source,
            output: Some(Box::new(output)),
        }),
        Err(error) => Err(error),
    }
}

/// Extracts retained bytes from a completed or failed output reader.
fn retained_output(
    result: Result<CapturedOutput, OutputCaptureError>,
) -> CapturedOutput {
    match result {
        Ok(output)
        | Err(OutputCaptureError::Read { output, .. })
        | Err(OutputCaptureError::Write { output, .. }) => output,
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
        OutputCaptureError::Read { source, output } => {
            let (stdout, stderr) = match stream {
                OutputStream::Stdout => {
                    (output, other_output.unwrap_or_default())
                }
                OutputStream::Stderr => {
                    (other_output.unwrap_or_default(), output)
                }
            };
            CommandError::ReadOutputFailed {
                command: command.to_owned(),
                stream,
                source,
                output: Some(Box::new(CommandOutput::new(
                    status,
                    (stdout.bytes, stdout.truncated, stdout.complete),
                    (stderr.bytes, stderr.truncated, stderr.complete),
                    elapsed,
                ))),
            }
        }
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
                    (stdout.bytes, stdout.truncated, stdout.complete),
                    (stderr.bytes, stderr.truncated, stderr.complete),
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
        Err(_) => Err(OutputCaptureError::Read {
            source: io::Error::other("output reader thread panicked"),
            output: CapturedOutput {
                bytes: Vec::new(),
                truncated: false,
                complete: false,
            },
        }),
    }
}
