// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;
use std::io::Read;
use std::io::Write;
use std::process::ExitStatus;
use std::thread;
use std::time::Duration;

use qubit_clock::TimeError;

use super::cancellable_reader::CancellableReader;
use super::captured_output::CapturedOutput;
use super::io_cancellation::IoCancellation;
use super::io_cancellation_token::IoCancellationToken;
use super::output_capture_error::OutputCaptureError;
use super::output_capture_failure::OutputCaptureFailure;
use super::output_capture_options::OutputCaptureOptions;
use super::output_reader::OutputReader;
use super::stdin_pipe::join_stdin_writer;
use super::stdin_writer::OptionalStdinWriter;
use crate::CommandCleanupFailure;
use crate::CommandError;
use crate::CommandErrorReason;
use crate::CommandOutput;
use crate::OutputStream;

/// Native descriptor used to poll a Unix output pipe.
#[cfg(unix)]
type OutputFd = std::os::fd::RawFd;
/// Placeholder descriptor type because Windows uses cancellable handles.
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
            let ready = cancellation
                .wait_for_fd(fd, libc::POLLIN)
                .map_err(|source| read_capture_error(source, bytes.clone(), truncated))?;
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
            Err(_source) if cancellation.is_some_and(IoCancellationToken::is_cancelled) => {
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
fn read_capture_error(source: io::Error, bytes: Vec<u8>, truncated: bool) -> OutputCaptureError {
    OutputCaptureError::Read {
        source,
        output: CapturedOutput {
            bytes,
            truncated,
            complete: false,
        },
    }
}

/// Reads one child output stream to completion for unit tests.
#[cfg(test)]
fn read_output(reader: &mut dyn Read, options: OutputCaptureOptions) -> Result<CapturedOutput, OutputCaptureError> {
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
    let (stdout, stdout_failure) = split_output_result(stdout_result);
    let (stderr, stderr_failure) = split_output_result(stderr_result);

    let stdin_error = stdin_result.err();
    let elapsed = match elapsed_result {
        Err(source) => {
            let mut cleanup_failures = Vec::new();
            if let Some(failure) = stdout_failure {
                cleanup_failures.push(output_cleanup_failure(OutputStream::Stdout, failure));
            }
            if let Some(failure) = stderr_failure {
                cleanup_failures.push(output_cleanup_failure(OutputStream::Stderr, failure));
            }
            if let Some(error) = stdin_error
                && let Some(failure) = error.into_cleanup_failure()
            {
                cleanup_failures.push(failure);
            }
            return Err(
                CommandError::from_reason(command, CommandErrorReason::TimeFailed { source }, None)
                    .with_cleanup_failures(cleanup_failures),
            );
        }
        Ok(elapsed) => elapsed,
    };

    if let Some(failure) = stdout_failure {
        let mut cleanup_failures = Vec::new();
        if let Some(failure) = stderr_failure {
            cleanup_failures.push(output_cleanup_failure(OutputStream::Stderr, failure));
        }
        if let Some(error) = stdin_error
            && let Some(failure) = error.into_cleanup_failure()
        {
            cleanup_failures.push(failure);
        }
        return Err(map_output_reader_error(
            command,
            status,
            elapsed,
            OutputStream::Stdout,
            failure,
            stdout,
            Some(stderr),
        )
        .with_cleanup_failures(cleanup_failures));
    }

    if let Some(failure) = stderr_failure {
        let mut cleanup_failures = Vec::new();
        if let Some(error) = stdin_error
            && let Some(failure) = error.into_cleanup_failure()
        {
            cleanup_failures.push(failure);
        }
        return Err(map_output_reader_error(
            command,
            status,
            elapsed,
            OutputStream::Stderr,
            failure,
            stderr,
            Some(stdout),
        )
        .with_cleanup_failures(cleanup_failures));
    }

    let output = CommandOutput::new(
        status,
        (stdout.bytes, stdout.truncated, stdout.complete),
        (stderr.bytes, stderr.truncated, stderr.complete),
        elapsed,
    );
    match stdin_error {
        None => Ok(output),
        Some(error) if matches!(error.kind(), crate::CommandErrorKind::WriteInputFailed) => {
            let command = error.command().to_owned();
            let source = match error.reason() {
                CommandErrorReason::WriteInputFailed { source } => io::Error::new(source.kind(), source.to_string()),
                _ => io::Error::other("invalid stdin error category"),
            };
            Err(CommandError::from_reason(
                command,
                CommandErrorReason::WriteInputFailed { source },
                Some(Box::new(output)),
            ))
        }
        Some(error) => Err(error),
    }
}

/// Separates retained bytes from an output-reader failure.
fn split_output_result(
    result: Result<CapturedOutput, OutputCaptureError>,
) -> (CapturedOutput, Option<OutputCaptureFailure>) {
    match result {
        Ok(output) => (output, None),
        Err(OutputCaptureError::Read { source, output }) => (output, Some(OutputCaptureFailure::Read { source })),
        Err(OutputCaptureError::Write { path, source, output }) => {
            (output, Some(OutputCaptureFailure::Write { path, source }))
        }
    }
}

/// Converts a reader failure into the public cleanup-failure category.
fn output_cleanup_failure(stream: OutputStream, failure: OutputCaptureFailure) -> CommandCleanupFailure {
    match (stream, failure) {
        (OutputStream::Stdout, OutputCaptureFailure::Read { source }) => CommandCleanupFailure::StdoutRead { source },
        (OutputStream::Stderr, OutputCaptureFailure::Read { source }) => CommandCleanupFailure::StderrRead { source },
        (OutputStream::Stdout, OutputCaptureFailure::Write { path, source }) => {
            CommandCleanupFailure::StdoutWrite { path, source }
        }
        (OutputStream::Stderr, OutputCaptureFailure::Write { path, source }) => {
            CommandCleanupFailure::StderrWrite { path, source }
        }
    }
}

/// Maps a reader result while retaining output from a failed tee write.
fn map_output_reader_error(
    command: &str,
    status: ExitStatus,
    elapsed: Duration,
    stream: OutputStream,
    error: OutputCaptureFailure,
    failed_output: CapturedOutput,
    other_output: Option<CapturedOutput>,
) -> CommandError {
    match error {
        OutputCaptureFailure::Read { source } => {
            let (stdout, stderr) = match stream {
                OutputStream::Stdout => (failed_output, other_output.unwrap_or_default()),
                OutputStream::Stderr => (other_output.unwrap_or_default(), failed_output),
            };
            CommandError::from_reason(
                command,
                CommandErrorReason::ReadOutputFailed { stream, source },
                Some(Box::new(CommandOutput::new(
                    status,
                    (stdout.bytes, stdout.truncated, stdout.complete),
                    (stderr.bytes, stderr.truncated, stderr.complete),
                    elapsed,
                ))),
            )
        }
        OutputCaptureFailure::Write { path, source } => {
            let (stdout, stderr) = match stream {
                OutputStream::Stdout => (failed_output, other_output.unwrap_or_default()),
                OutputStream::Stderr => (other_output.unwrap_or_default(), failed_output),
            };
            CommandError::from_reason(
                command,
                CommandErrorReason::WriteOutputFailed { stream, path, source },
                Some(Box::new(CommandOutput::new(
                    status,
                    (stdout.bytes, stdout.truncated, stdout.complete),
                    (stderr.bytes, stderr.truncated, stderr.complete),
                    elapsed,
                ))),
            )
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
/// Returns a [`CommandError`] with kind `ReadOutputFailed` for read failures or
/// thread panics, and kind `WriteOutputFailed` for tee failures.
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Cursor;
    use std::io::Read;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::path::Path;
    use std::process::ExitStatus;
    use std::thread;
    use std::time::Duration;

    use qubit_clock::TimeError;
    use qubit_clock::TimerUnavailableError;

    use super::super::captured_output::CapturedOutput;
    use super::super::io_cancellation::IoCancellation;
    use super::super::output_capture_error::OutputCaptureError;
    use super::super::output_capture_options::OutputCaptureOptions;
    use super::super::output_reader::OutputReader;
    use super::super::output_tee::OutputTee;
    use super::collect_output_results;
    use super::join_output_reader;
    use super::read_output;
    use crate::CommandCleanupFailure;
    use crate::CommandError;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;
    use crate::OutputStream;

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code as u32)
    }

    fn captured(bytes: &[u8], truncated: bool, complete: bool) -> CapturedOutput {
        CapturedOutput {
            bytes: bytes.to_vec(),
            truncated,
            complete,
        }
    }

    fn stdin_failure() -> CommandError {
        CommandError::from_reason(
            "command",
            CommandErrorReason::WriteInputFailed {
                source: io::Error::other("injected stdin write failure"),
            },
            None,
        )
    }

    struct FailingReader {
        prefix: Cursor<Vec<u8>>,
    }

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = self.prefix.read(buffer)?;
            if read == 0 {
                Err(io::Error::other("injected read failure"))
            } else {
                Ok(read)
            }
        }
    }

    struct FailingWriter {
        fail_write: bool,
    }

    impl Write for FailingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self.fail_write {
                Err(io::Error::other("write failure"))
            } else {
                Ok(buffer.len())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.fail_write {
                Ok(())
            } else {
                Err(io::Error::other("flush failure"))
            }
        }
    }

    #[test]
    fn test_output_collector_read_failure_preserves_partial_output() {
        let error = read_output(
            &mut FailingReader {
                prefix: Cursor::new(b"partial".to_vec()),
            },
            OutputCaptureOptions::new(None, None),
        )
        .expect_err("read failure should be returned");

        let OutputCaptureError::Read { output, .. } = error else {
            panic!("expected output read failure");
        };
        assert_eq!(output.bytes, b"partial");
        assert!(!output.complete);
    }

    #[test]
    fn test_output_collector_write_failure_preserves_tee_path() {
        let error = read_output(
            &mut Cursor::new(b"output"),
            OutputCaptureOptions::new(
                None,
                Some(OutputTee::new(
                    Box::new(FailingWriter { fail_write: true }),
                    "tee-write.log".into(),
                )),
            ),
        )
        .expect_err("write failure should be returned");

        let OutputCaptureError::Write { path, .. } = error else {
            panic!("expected tee write failure");
        };
        assert_eq!(path, Path::new("tee-write.log"));
    }

    #[test]
    fn test_output_collector_flush_failure_preserves_tee_path() {
        let error = read_output(
            &mut Cursor::new(b"output"),
            OutputCaptureOptions::new(
                None,
                Some(OutputTee::new(
                    Box::new(FailingWriter { fail_write: false }),
                    "tee-flush.log".into(),
                )),
            ),
        )
        .expect_err("flush failure should be returned");

        let OutputCaptureError::Write { path, .. } = error else {
            panic!("expected tee flush failure");
        };
        assert_eq!(path, Path::new("tee-flush.log"));
    }

    #[test]
    fn test_join_output_reader_maps_worker_panic() {
        let (cancellation, token) = IoCancellation::pair().expect("cancellation pair should be created");
        let reader = OutputReader::new(
            thread::spawn(move || -> Result<CapturedOutput, OutputCaptureError> {
                let _token = token;
                panic!("injected output worker panic");
            }),
            cancellation,
        );

        let error = join_output_reader(reader).expect_err("output worker panic should be mapped");

        let OutputCaptureError::Read { source, output } = error else {
            panic!("expected output read failure");
        };
        assert_eq!(source.to_string(), "output reader thread panicked");
        assert!(!output.complete);
    }

    #[test]
    fn test_collect_output_prioritizes_elapsed_failure_and_retains_helpers() {
        let elapsed_error = TimeError::TimerUnavailable {
            source: TimerUnavailableError::BackendUnavailable {
                backend: "test",
                source: Box::new(io::Error::other("injected elapsed failure")),
            },
        };
        let stdout_error = OutputCaptureError::Read {
            source: io::Error::other("injected stdout read failure"),
            output: CapturedOutput::default(),
        };
        let stderr_error = OutputCaptureError::Write {
            path: "stderr.log".into(),
            source: io::Error::other("injected stderr write failure"),
            output: CapturedOutput::default(),
        };
        let stdin_error = CommandError::from_reason(
            "command",
            CommandErrorReason::WriteInputFailed {
                source: io::Error::other("injected stdin write failure"),
            },
            None,
        );

        let error = collect_output_results(
            "command",
            status(0),
            Err(elapsed_error),
            Err(stdout_error),
            Err(stderr_error),
            Err(stdin_error),
        )
        .expect_err("elapsed failure should remain primary");

        assert_eq!(error.kind(), CommandErrorKind::TimeFailed);
        assert!(matches!(
            error.cleanup_failures(),
            [
                CommandCleanupFailure::StdoutRead { .. },
                CommandCleanupFailure::StderrWrite { .. },
                CommandCleanupFailure::Stdin { .. },
            ]
        ));
        assert!(error.output().is_none());
    }

    #[test]
    fn test_collect_output_maps_stdout_read_failure_with_partial_streams() {
        let error = collect_output_results(
            "command",
            status(0),
            Ok(Duration::from_secs(1)),
            Err(OutputCaptureError::Read {
                source: io::Error::other("injected stdout read failure"),
                output: captured(b"partial-stdout", false, false),
            }),
            Ok(captured(b"complete-stderr", false, true)),
            Ok(()),
        )
        .expect_err("stdout read failure should be mapped");

        assert!(matches!(
            error.reason(),
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stdout,
                ..
            }
        ));
        let output = error.output().expect("partial output should be retained");
        assert_eq!(output.stdout(), b"partial-stdout");
        assert_eq!(output.stderr(), b"complete-stderr");
        assert!(!output.stdout_complete());
        assert!(output.stderr_complete());
    }

    #[test]
    fn test_collect_output_orders_combined_helper_failures() {
        let error = collect_output_results(
            "command",
            status(0),
            Ok(Duration::from_secs(1)),
            Err(OutputCaptureError::Read {
                source: io::Error::other("injected stdout read failure"),
                output: CapturedOutput::default(),
            }),
            Err(OutputCaptureError::Write {
                path: "stderr.log".into(),
                source: io::Error::other("injected stderr tee failure"),
                output: CapturedOutput::default(),
            }),
            Err(stdin_failure()),
        )
        .expect_err("stdout failure should remain primary");

        assert!(matches!(
            error.reason(),
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stdout,
                ..
            }
        ));
        assert!(matches!(
            error.cleanup_failures(),
            [
                CommandCleanupFailure::StderrWrite { .. },
                CommandCleanupFailure::Stdin { .. },
            ]
        ));
    }

    #[test]
    fn test_collect_output_maps_stderr_read_failure() {
        let error = collect_output_results(
            "command",
            status(0),
            Ok(Duration::from_secs(1)),
            Ok(captured(b"stdout", false, true)),
            Err(OutputCaptureError::Read {
                source: io::Error::other("injected stderr read failure"),
                output: captured(b"partial-stderr", false, false),
            }),
            Ok(()),
        )
        .expect_err("stderr read failure should be mapped");

        assert!(matches!(
            error.reason(),
            CommandErrorReason::ReadOutputFailed {
                stream: OutputStream::Stderr,
                ..
            }
        ));
        let output = error.output().expect("partial output should be retained");
        assert_eq!(output.stdout(), b"stdout");
        assert_eq!(output.stderr(), b"partial-stderr");
    }

    #[test]
    fn test_collect_output_maps_each_tee_failure() {
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            let failure = OutputCaptureError::Write {
                path: format!("{stream}.log").into(),
                source: io::Error::other("injected tee failure"),
                output: captured(b"output", false, true),
            };
            let (stdout, stderr) = match stream {
                OutputStream::Stdout => (Err(failure), Ok(CapturedOutput::default())),
                OutputStream::Stderr => (Ok(CapturedOutput::default()), Err(failure)),
            };

            let error =
                collect_output_results("command", status(0), Ok(Duration::from_secs(1)), stdout, stderr, Ok(()))
                    .expect_err("tee failure should be mapped");

            assert!(matches!(
                error.reason(),
                CommandErrorReason::WriteOutputFailed {
                    stream: actual,
                    ..
                } if *actual == stream
            ));
        }
    }

    #[test]
    fn test_collect_output_preserves_completed_output_on_stdin_failure() {
        let error = collect_output_results(
            "command",
            status(0),
            Ok(Duration::from_secs(1)),
            Ok(captured(b"stdout", false, true)),
            Ok(captured(b"stderr", false, true)),
            Err(stdin_failure()),
        )
        .expect_err("stdin failure should be mapped");

        assert_eq!(error.kind(), CommandErrorKind::WriteInputFailed);
        let output = error.output().expect("completed output should be retained");
        assert_eq!(output.stdout(), b"stdout");
        assert_eq!(output.stderr(), b"stderr");
    }

    #[test]
    fn test_collect_output_builds_success_with_truncation() {
        let output = collect_output_results(
            "command",
            status(0),
            Ok(Duration::from_secs(1)),
            Ok(captured(b"stdout", true, true)),
            Ok(captured(b"stderr", false, true)),
            Ok(()),
        )
        .expect("successful helper results should produce output");

        assert_eq!(output.stdout(), b"stdout");
        assert_eq!(output.stderr(), b"stderr");
        assert!(output.stdout_truncated());
    }
}
