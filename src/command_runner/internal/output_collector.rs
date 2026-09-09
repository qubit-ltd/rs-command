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
/// Every exit finalizes pending tee errors. A later pipe failure is retained
/// alongside the earlier tee error; cancellation never erases either stored
/// failure. Bytes are captured before tee I/O, and completeness records pipe
/// EOF independently of tee write or flush success.
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
/// Returns an output read or tee write error, or both when draining a failed
/// tee subsequently encounters a pipe error. All variants retain captured
/// bytes and stream metadata.
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
    let read_result = loop {
        if cancellation.is_some_and(IoCancellationToken::is_cancelled) {
            break Ok(false);
        }
        #[cfg(unix)]
        if let (Some(cancellation), Some(fd)) = (cancellation, fd) {
            match cancellation.wait_for_fd(fd, libc::POLLIN) {
                Ok(true) => {}
                Ok(false) => break Ok(false),
                Err(source) => break Err(source),
            }
        }
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                if cancellation.is_some_and(IoCancellationToken::is_cancelled) {
                    break Ok(false);
                }
                if cancellation.is_none() {
                    thread::sleep(Duration::from_millis(1));
                }
                continue;
            }
            Err(_source) if cancellation.is_some_and(IoCancellationToken::is_cancelled) => {
                break Ok(false);
            }
            Err(source) => {
                break Err(source);
            }
        };
        if read == 0 {
            break Ok(true);
        }
        let chunk = &buffer[..read];
        // Capture bytes before tee I/O so an interrupted write cannot discard
        // data that has already been consumed from the child pipe.
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
        if let Some(tee) = options.tee.as_mut()
            && let Err(source) = tee.writer.write_all(chunk)
        {
            write_error = Some((tee.path.clone(), source));
            options.tee = None;
        }
    };
    let complete = matches!(read_result, Ok(true));
    if complete
        && let Some(tee) = options.tee.as_mut()
        && let Err(source) = tee.writer.flush()
    {
        write_error = Some((tee.path.clone(), source));
    }
    let output = CapturedOutput {
        bytes,
        truncated,
        complete,
    };
    match (write_error, read_result) {
        (Some((path, write_source)), Err(read_source)) => Err(OutputCaptureError::ReadAfterWrite {
            path,
            write_source,
            read_source,
            output,
        }),
        (Some((path, source)), Ok(_)) => Err(OutputCaptureError::Write { path, source, output }),
        (None, Err(source)) => Err(OutputCaptureError::Read { source, output }),
        (None, Ok(_)) => Ok(output),
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
    let (stdout, stdout_failures) = split_output_result(stdout_result);
    let (stderr, stderr_failures) = split_output_result(stderr_result);
    let mut stdout_failures = stdout_failures.into_iter();
    let mut stderr_failures = stderr_failures.into_iter();

    let stdin_error = stdin_result.err();
    let elapsed = match elapsed_result {
        Err(source) => {
            let mut cleanup_failures = Vec::new();
            cleanup_failures
                .extend(stdout_failures.map(|failure| output_cleanup_failure(OutputStream::Stdout, failure)));
            cleanup_failures
                .extend(stderr_failures.map(|failure| output_cleanup_failure(OutputStream::Stderr, failure)));
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

    if let Some(failure) = stdout_failures.next() {
        let mut cleanup_failures = Vec::new();
        cleanup_failures.extend(stdout_failures.map(|failure| output_cleanup_failure(OutputStream::Stdout, failure)));
        cleanup_failures.extend(stderr_failures.map(|failure| output_cleanup_failure(OutputStream::Stderr, failure)));
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

    if let Some(failure) = stderr_failures.next() {
        let mut cleanup_failures = Vec::new();
        cleanup_failures.extend(stderr_failures.map(|failure| output_cleanup_failure(OutputStream::Stderr, failure)));
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
            Err(error.with_output(output))
        }
        Some(error) => Err(error),
    }
}

/// Separates retained bytes from all failures observed by one output reader.
///
/// # Parameters
///
/// * `result` - Completed reader result, including any prior tee failure.
///
/// # Returns
///
/// Retained output and failures in occurrence order; successful reads return
/// an empty vector without allocating.
pub(super) fn split_output_result(
    result: Result<CapturedOutput, OutputCaptureError>,
) -> (CapturedOutput, Vec<OutputCaptureFailure>) {
    match result {
        Ok(output) => (output, Vec::new()),
        Err(OutputCaptureError::Read { source, output }) => (output, vec![OutputCaptureFailure::Read { source }]),
        Err(OutputCaptureError::Write { path, source, output }) => {
            (output, vec![OutputCaptureFailure::Write { path, source }])
        }
        Err(OutputCaptureError::ReadAfterWrite {
            path,
            write_source,
            read_source,
            output,
        }) => (
            output,
            vec![
                OutputCaptureFailure::Write {
                    path,
                    source: write_source,
                },
                OutputCaptureFailure::Read { source: read_source },
            ],
        ),
    }
}

/// Converts a reader failure into the public cleanup-failure category.
///
/// # Parameters
///
/// * `stream` - Stream that produced the failure.
/// * `failure` - Owned failure whose original I/O source must be preserved.
///
/// # Returns
///
/// A stream-specific cleanup failure with its original path and source.
pub(super) fn output_cleanup_failure(stream: OutputStream, failure: OutputCaptureFailure) -> CommandCleanupFailure {
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
    use std::os::fd::AsRawFd;
    #[cfg(unix)]
    use std::os::unix::net::UnixStream;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::path::Path;
    use std::process::ExitStatus;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    #[cfg(unix)]
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use qubit_clock::TimeError;
    use qubit_clock::TimerUnavailableError;

    use super::super::captured_output::CapturedOutput;
    use super::super::command_io::CommandIo;
    use super::super::io_cancellation::IoCancellation;
    use super::super::output_capture_error::OutputCaptureError;
    use super::super::output_capture_options::OutputCaptureOptions;
    use super::super::output_reader::OutputReader;
    use super::super::output_tee::OutputTee;
    use super::super::stop_reason::StopReason;
    use super::collect_output_results;
    use super::join_output_reader;
    use super::read_output;
    use super::read_output_inner;
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
                source: io::Error::from_raw_os_error(7),
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

    /// Chooses the observation immediately after the first tee write fails.
    #[derive(Clone, Copy, Debug)]
    enum AfterTeeFailure {
        CancelWithBytes,
        CancelInterrupted,
        CancelWouldBlock,
        CancelReadError,
        ReadError,
        Eof,
    }

    /// Produces a prefix, then cancels or fails without relying on scheduling.
    struct ReaderAfterTeeFailure {
        prefix_sent: bool,
        cancelled: Arc<AtomicBool>,
        outcome: AfterTeeFailure,
    }

    impl Read for ReaderAfterTeeFailure {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.prefix_sent {
                self.prefix_sent = true;
                buffer[..6].copy_from_slice(b"prefix");
                return Ok(6);
            }
            match self.outcome {
                AfterTeeFailure::ReadError => return Err(io::Error::from_raw_os_error(9)),
                AfterTeeFailure::Eof => return Ok(0),
                _ => self.cancelled.store(true, Ordering::Release),
            }
            match self.outcome {
                AfterTeeFailure::CancelWithBytes => {
                    buffer[..6].copy_from_slice(b"suffix");
                    Ok(6)
                }
                AfterTeeFailure::CancelInterrupted => Err(io::ErrorKind::Interrupted.into()),
                AfterTeeFailure::CancelWouldBlock => Err(io::ErrorKind::WouldBlock.into()),
                _ => Err(io::Error::from_raw_os_error(9)),
            }
        }
    }

    /// Captures a stream with a failed tee followed by one terminal event.
    fn capture_after_tee_failure(outcome: AfterTeeFailure) -> Result<CapturedOutput, OutputCaptureError> {
        let (_sender, token) = IoCancellation::pair().expect("cancellation pair must initialize");
        let mut reader = ReaderAfterTeeFailure {
            prefix_sent: false,
            cancelled: Arc::clone(&token.cancelled),
            outcome,
        };
        read_output_inner(
            &mut reader,
            OutputCaptureOptions::new(
                Some(8),
                Some(OutputTee::new(
                    Box::new(FailingWriter { fail_write: true }),
                    "failed-tee.log".into(),
                )),
            ),
            Some(&token),
            None,
        )
    }

    #[test]
    fn test_pending_tee_error_survives_every_read_cancellation() {
        for outcome in [
            AfterTeeFailure::CancelWithBytes,
            AfterTeeFailure::CancelInterrupted,
            AfterTeeFailure::CancelWouldBlock,
            AfterTeeFailure::CancelReadError,
        ] {
            let error = capture_after_tee_failure(outcome).expect_err("pending tee error must survive cancellation");
            let OutputCaptureError::Write { path, source, output } = error else {
                panic!("expected retained tee error for {outcome:?}");
            };
            assert_eq!(path, Path::new("failed-tee.log"));
            assert_eq!(source.to_string(), "write failure");
            assert!(!output.complete, "cancelled stream must remain incomplete");
            if matches!(outcome, AfterTeeFailure::CancelWithBytes) {
                assert_eq!(output.bytes, b"prefixsu");
                assert!(output.truncated);
            } else {
                assert_eq!(output.bytes, b"prefix");
                assert!(!output.truncated);
            }
        }
    }

    #[test]
    fn test_pending_tee_error_survives_later_read_failure() {
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            let failed = capture_after_tee_failure(AfterTeeFailure::ReadError);
            let (stdout, stderr) = match stream {
                OutputStream::Stdout => (failed, Ok(CapturedOutput::default())),
                OutputStream::Stderr => (Ok(CapturedOutput::default()), failed),
            };
            let error =
                collect_output_results("command", status(0), Ok(Duration::from_secs(1)), stdout, stderr, Ok(()))
                    .expect_err("both output failures must be reported");
            assert!(matches!(
                error.reason(),
                CommandErrorReason::WriteOutputFailed { stream: actual, source, .. }
                    if *actual == stream && source.to_string() == "write failure"
            ));
            assert!(matches!(
                (stream, error.cleanup_failures()),
                (OutputStream::Stdout, [CommandCleanupFailure::StdoutRead { source }])
                | (OutputStream::Stderr, [CommandCleanupFailure::StderrRead { source }])
                    if source.raw_os_error() == Some(9)
            ));
            let output = error.output().expect("partial output must be retained");
            match stream {
                OutputStream::Stdout => {
                    assert_eq!(output.stdout(), b"prefix");
                    assert!(!output.stdout_complete());
                }
                OutputStream::Stderr => {
                    assert_eq!(output.stderr(), b"prefix");
                    assert!(!output.stderr_complete());
                }
            }
        }
    }

    /// Checks that demotion keeps all four stream errors in resource order.
    fn assert_both_stream_failures(failures: &[CommandCleanupFailure]) {
        assert!(matches!(
            failures,
            [
                CommandCleanupFailure::StdoutWrite { path: stdout_path, source: stdout_write },
                CommandCleanupFailure::StdoutRead { source: stdout_read },
                CommandCleanupFailure::StderrWrite { path: stderr_path, source: stderr_write },
                CommandCleanupFailure::StderrRead { source: stderr_read },
            ] if stdout_path == Path::new("failed-tee.log")
                && stderr_path == Path::new("failed-tee.log")
                && stdout_write.to_string() == "write failure"
                && stderr_write.to_string() == "write failure"
                && stdout_read.raw_os_error() == Some(9)
                && stderr_read.raw_os_error() == Some(9)
        ));
    }

    #[test]
    fn test_combined_stream_failures_survive_time_failure_and_stop_reason_demotion() {
        let error = collect_output_results(
            "command",
            status(0),
            Err(TimeError::InstantOverflow),
            capture_after_tee_failure(AfterTeeFailure::ReadError),
            capture_after_tee_failure(AfterTeeFailure::ReadError),
            Ok(()),
        )
        .expect_err("clock error must retain both stream failures");
        assert_eq!(error.kind(), CommandErrorKind::TimeFailed);
        assert!(error.output().is_none());
        assert_both_stream_failures(error.cleanup_failures());

        for reason in [
            StopReason::TimedOut {
                timeout: Duration::from_secs(1),
                status: Some(status(0)),
            },
            StopReason::Cancelled {
                status: Some(status(0)),
            },
        ] {
            let error = collect_output_results(
                "command",
                status(0),
                Ok(Duration::from_secs(1)),
                capture_after_tee_failure(AfterTeeFailure::ReadError),
                capture_after_tee_failure(AfterTeeFailure::ReadError),
                Ok(()),
            )
            .expect_err("combined stream failures must be retained");
            let expected_kind = if matches!(reason, StopReason::TimedOut { .. }) {
                CommandErrorKind::TimedOut
            } else {
                CommandErrorKind::Cancelled
            };
            let error = reason.into_error_after_finalize("command", error);
            assert_eq!(error.kind(), expected_kind);
            assert_both_stream_failures(error.cleanup_failures());
            let output = error.output().expect("demotion must preserve both partial streams");
            assert_eq!(output.stdout(), b"prefix");
            assert_eq!(output.stderr(), b"prefix");
            assert!(!output.stdout_complete());
            assert!(!output.stderr_complete());
        }
    }

    /// Runs the real collector fault sequence inside an owned helper thread.
    fn failed_output_reader() -> OutputReader {
        let (sender, token) = IoCancellation::pair().expect("cancellation pair must initialize");
        OutputReader::new(
            thread::spawn(move || {
                let _token = token;
                capture_after_tee_failure(AfterTeeFailure::ReadError)
            }),
            sender,
        )
    }

    #[test]
    fn test_combined_stream_failures_survive_cleanup_without_process_status() {
        let io = CommandIo::new(failed_output_reader(), failed_output_reader(), None);
        let failures = io.cancel_and_join("command");
        assert_both_stream_failures(&failures);
    }

    /// Signals only after the collector stores the failed tee and drops it.
    #[cfg(unix)]
    struct NotifyingFailedTee(mpsc::Sender<()>);

    #[cfg(unix)]
    impl Write for NotifyingFailedTee {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("pipe tee failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(unix)]
    impl Drop for NotifyingFailedTee {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_pending_tee_error_survives_pipe_wakeup_cancellation() {
        let (mut input, mut peer) = UnixStream::pair().expect("test pipe must initialize");
        input.set_nonblocking(true).expect("test pipe must be nonblocking");
        peer.write_all(b"prefix").expect("test prefix must be written");
        let (sender, token) = IoCancellation::pair().expect("cancellation pair must initialize");
        let (failed, observed) = mpsc::channel();
        let join = thread::spawn(move || {
            let fd = input.as_raw_fd();
            read_output_inner(
                &mut input,
                OutputCaptureOptions::new(
                    Some(4),
                    Some(OutputTee::new(
                        Box::new(NotifyingFailedTee(failed)),
                        "pipe-tee.log".into(),
                    )),
                ),
                Some(&token),
                Some(fd),
            )
        });
        let failure_observed = observed.recv_timeout(Duration::from_secs(5));
        let cancellation = sender.cancel(&join);
        // Closing the peer also releases the helper if the OS rejects the
        // cancellation request, so even a failed assertion cannot leak it.
        if cancellation.is_err() || failure_observed.is_err() {
            drop(peer);
        }
        let result = join.join().expect("output helper must not panic");
        failure_observed.expect("tee failure must be stored before cancellation");
        cancellation.expect("pipe reader must be cancellable");
        let OutputCaptureError::Write { path, source, output } = result.expect_err("pending tee failure must survive")
        else {
            panic!("expected tee failure after pipe cancellation");
        };
        assert_eq!(path, Path::new("pipe-tee.log"));
        assert_eq!(source.to_string(), "pipe tee failure");
        assert_eq!(output.bytes, b"pref");
        assert!(output.truncated);
        assert!(!output.complete);
    }

    #[test]
    fn test_pending_tee_error_keeps_eof_complete() {
        let error = capture_after_tee_failure(AfterTeeFailure::Eof).expect_err("tee failure must survive EOF");
        let OutputCaptureError::Write { output, .. } = error else {
            panic!("expected tee failure at EOF");
        };
        assert_eq!(output.bytes, b"prefix");
        assert!(output.complete, "tee failure does not undo observed pipe EOF");
    }

    /// Cancels exactly inside a failing write or flush operation.
    struct CancellingWriter {
        cancelled: Arc<AtomicBool>,
        fail_on_flush: bool,
    }

    impl Write for CancellingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail_on_flush {
                return Ok(bytes.len());
            }
            self.cancelled.store(true, Ordering::Release);
            Err(io::Error::other("cancelled tee write"))
        }

        fn flush(&mut self) -> io::Result<()> {
            self.cancelled.store(true, Ordering::Release);
            Err(io::Error::other("cancelled tee flush"))
        }
    }

    #[test]
    fn test_tee_cancellation_preserves_read_bytes_and_observed_eof() {
        for fail_on_flush in [false, true] {
            let (_sender, token) = IoCancellation::pair().expect("cancellation pair must initialize");
            let error = read_output_inner(
                &mut Cursor::new(b"output"),
                OutputCaptureOptions::new(
                    Some(4),
                    Some(OutputTee::new(
                        Box::new(CancellingWriter {
                            cancelled: Arc::clone(&token.cancelled),
                            fail_on_flush,
                        }),
                        "cancelled-tee.log".into(),
                    )),
                ),
                Some(&token),
                None,
            )
            .expect_err("tee failure must survive simultaneous cancellation");
            let OutputCaptureError::Write { path, source, output } = error else {
                panic!("expected tee write or flush failure");
            };
            assert_eq!(path, Path::new("cancelled-tee.log"));
            assert_eq!(
                source.to_string(),
                if fail_on_flush {
                    "cancelled tee flush"
                } else {
                    "cancelled tee write"
                }
            );
            assert_eq!(
                output.bytes, b"outp",
                "bytes already read must survive tee cancellation"
            );
            assert!(output.truncated);
            assert_eq!(
                output.complete, fail_on_flush,
                "completeness follows pipe EOF, not tee success"
            );
        }
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
    fn test_collect_output_preserves_stdin_os_error_source() {
        let error = collect_output_results(
            "command",
            status(0),
            Ok(Duration::from_secs(1)),
            Ok(CapturedOutput::default()),
            Ok(CapturedOutput::default()),
            Err(stdin_failure()),
        )
        .expect_err("stdin failure should be mapped");

        let CommandErrorReason::WriteInputFailed { source } = error.reason() else {
            panic!("expected stdin write failure");
        };
        assert_eq!(source.raw_os_error(), Some(7));
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
