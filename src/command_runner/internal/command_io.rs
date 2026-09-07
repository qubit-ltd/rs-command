// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::thread;
use std::time::Duration;
use std::time::Instant;

use qubit_clock::TimeError;

use super::captured_output::CapturedOutput;
use super::output_capture_error::OutputCaptureError;
use super::output_collector::collect_output;
use super::output_collector::collect_output_results;
use super::output_collector::join_output_reader;
use super::output_reader::OutputReader;
use super::stdin_pipe::join_stdin_writer;
use super::stdin_writer::OptionalStdinWriter;
use crate::CommandCleanupFailure;
use crate::CommandError;
use crate::CommandOutput;

/// Maximum time spent confirming that a helper stopped after cancellation
/// itself failed.
const HELPER_CANCELLATION_CONFIRMATION_TIMEOUT: Duration = Duration::from_millis(100);

/// Delay between checks while confirming a failed helper cancellation.
const HELPER_CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Output and stdin helper threads for one running command.
#[must_use = "command I/O owns helper threads that must be collected"]
pub(in crate::command_runner) struct CommandIo {
    /// Reader thread draining stdout.
    stdout_reader: OutputReader,
    /// Reader thread draining stderr.
    stderr_reader: OutputReader,
    /// Optional writer thread feeding stdin.
    stdin_writer: OptionalStdinWriter,
}

impl CommandIo {
    /// Creates a command I/O helper bundle.
    ///
    /// # Parameters
    ///
    /// * `stdout_reader` - Reader thread draining stdout.
    /// * `stderr_reader` - Reader thread draining stderr.
    /// * `stdin_writer` - Optional writer thread feeding stdin.
    ///
    /// # Returns
    ///
    /// I/O helper bundle consumed when output is collected or drained.
    #[inline]
    pub(in crate::command_runner) fn new(
        stdout_reader: OutputReader,
        stderr_reader: OutputReader,
        stdin_writer: OptionalStdinWriter,
    ) -> Self {
        Self {
            stdout_reader,
            stderr_reader,
            stdin_writer,
        }
    }

    /// Returns whether all helper threads have finished.
    ///
    /// # Returns
    ///
    /// `true` when stdout, stderr, and optional stdin helpers can be joined
    /// without blocking.
    #[must_use]
    #[inline]
    pub(in crate::command_runner) fn is_finished(&self) -> bool {
        self.stdout_reader.is_finished()
            && self.stderr_reader.is_finished()
            && self
                .stdin_writer
                .as_ref()
                .is_none_or(|writer| writer.is_finished())
    }

    /// Collects output from all helper threads.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `status` - Process exit status.
    /// * `elapsed` - Callback that samples command duration after all helper
    ///   threads have finished.
    ///
    /// # Returns
    ///
    /// Captured command output.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if stream collection or stdin writing fails.
    #[must_use]
    #[inline(always)]
    pub(in crate::command_runner) fn collect<F>(
        self,
        command: &str,
        status: std::process::ExitStatus,
        elapsed: F,
    ) -> Result<CommandOutput, CommandError>
    where
        F: FnOnce() -> Result<Duration, TimeError>,
    {
        collect_output(
            command,
            status,
            elapsed,
            self.stdout_reader,
            self.stderr_reader,
            self.stdin_writer,
        )
    }

    /// Cancels and joins every helper after process termination.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `status` - Process exit status.
    /// * `elapsed` - Callback that samples command duration after helpers have
    ///   been cancelled and joined.
    ///
    /// # Returns
    ///
    /// Captured output plus cancellation failures in stdout/stderr/stdin
    /// order. Interrupted streams whose helper could not be joined are marked
    /// incomplete.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if a completed helper or elapsed-time sampling
    /// failed.
    #[must_use]
    pub(in crate::command_runner) fn cancel_and_collect<F>(
        self,
        command: &str,
        status: std::process::ExitStatus,
        elapsed: F,
    ) -> (
        Result<CommandOutput, CommandError>,
        Vec<CommandCleanupFailure>,
    )
    where
        F: FnOnce() -> Result<Duration, TimeError>,
    {
        self.finish_helpers(
            command,
            move |stdout_result, stderr_result, stdin_result, cleanup_failures| {
                let output = collect_output_results(
                    command,
                    status,
                    elapsed(),
                    stdout_result.unwrap_or_else(detached_output),
                    stderr_result.unwrap_or_else(detached_output),
                    stdin_result.unwrap_or(Ok(())),
                );
                (output, cleanup_failures)
            },
        )
    }

    /// Cancels and joins every helper without process status.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    ///
    /// # Returns
    ///
    /// All helper failures in stdout/stderr/stdin order after all joins
    /// complete.
    #[must_use]
    pub(in crate::command_runner) fn cancel_and_join(
        self,
        command: &str,
    ) -> Vec<CommandCleanupFailure> {
        self.finish_helpers(
            command,
            |stdout_result, stderr_result, stdin_result, mut failures| {
                match stdout_result {
                    None | Some(Ok(_)) => {}
                    Some(Err(OutputCaptureError::Read { source, .. })) => {
                        failures.push(CommandCleanupFailure::StdoutRead { source });
                    }
                    Some(Err(OutputCaptureError::Write { path, source, .. })) => {
                        failures.push(CommandCleanupFailure::StdoutWrite { path, source });
                    }
                }
                match stderr_result {
                    None | Some(Ok(_)) => {}
                    Some(Err(OutputCaptureError::Read { source, .. })) => {
                        failures.push(CommandCleanupFailure::StderrRead { source });
                    }
                    Some(Err(OutputCaptureError::Write { path, source, .. })) => {
                        failures.push(CommandCleanupFailure::StderrWrite { path, source });
                    }
                }
                if let Some(Err(error)) = stdin_result
                    && let Some(failure) = error.into_cleanup_failure()
                {
                    failures.push(failure);
                }
                failures
            },
        )
    }

    /// Cancels every helper, bounds confirmation after cancellation failures,
    /// and then either collects or drains the available results.
    fn finish_helpers<R>(
        self,
        command: &str,
        finish: impl FnOnce(
            Option<Result<CapturedOutput, OutputCaptureError>>,
            Option<Result<CapturedOutput, OutputCaptureError>>,
            Option<Result<(), CommandError>>,
            Vec<CommandCleanupFailure>,
        ) -> R,
    ) -> R {
        let Self {
            stdout_reader,
            stderr_reader,
            stdin_writer,
        } = self;

        // Issue every request before waiting for any helper so one failure
        // cannot prevent the other helpers from being interrupted.
        let confirmation_deadline = Instant::now() + HELPER_CANCELLATION_CONFIRMATION_TIMEOUT;
        let stdout_cancellation = stdout_reader.cancel().err();
        let stderr_cancellation = stderr_reader.cancel().err();
        let stdin_cancellation = stdin_writer
            .as_ref()
            .and_then(|writer| writer.cancel().err());

        let stdout_result = finish_output_reader(
            stdout_reader,
            stdout_cancellation.is_some(),
            confirmation_deadline,
        );
        let stderr_result = finish_output_reader(
            stderr_reader,
            stderr_cancellation.is_some(),
            confirmation_deadline,
        );
        let stdin_result = finish_stdin_writer(
            command,
            stdin_writer,
            stdin_cancellation.is_some(),
            confirmation_deadline,
        );

        let mut cancellation_failures = Vec::new();
        if let Some(source) = stdout_cancellation {
            cancellation_failures.push(CommandCleanupFailure::StdoutCancellation { source });
        }
        if let Some(source) = stderr_cancellation {
            cancellation_failures.push(CommandCleanupFailure::StderrCancellation { source });
        }
        if let Some(source) = stdin_cancellation {
            cancellation_failures.push(CommandCleanupFailure::StdinCancellation { source });
        }

        finish(
            stdout_result,
            stderr_result,
            stdin_result,
            cancellation_failures,
        )
    }
}

/// Joins a reader unless a failed cancellation remains unconfirmed.
fn finish_output_reader(
    reader: OutputReader,
    cancellation_failed: bool,
    confirmation_deadline: Instant,
) -> Option<Result<CapturedOutput, OutputCaptureError>> {
    if cancellation_failed && !wait_until_finished(|| reader.is_finished(), confirmation_deadline) {
        return None;
    }
    Some(join_output_reader(reader))
}

/// Joins an optional writer unless a failed cancellation remains unconfirmed.
fn finish_stdin_writer(
    command: &str,
    writer: OptionalStdinWriter,
    cancellation_failed: bool,
    confirmation_deadline: Instant,
) -> Option<Result<(), CommandError>> {
    if cancellation_failed
        && let Some(writer) = writer.as_ref()
        && !wait_until_finished(|| writer.is_finished(), confirmation_deadline)
    {
        return None;
    }
    Some(join_stdin_writer(command, writer))
}

/// Waits only until the shared cancellation-confirmation deadline.
fn wait_until_finished(is_finished: impl Fn() -> bool, confirmation_deadline: Instant) -> bool {
    while !is_finished() {
        let now = Instant::now();
        if now >= confirmation_deadline {
            return false;
        }
        thread::sleep(
            HELPER_CANCELLATION_POLL_INTERVAL
                .min(confirmation_deadline.saturating_duration_since(now)),
        );
    }
    true
}

/// Represents output unavailable because its helper was detached.
fn detached_output() -> Result<CapturedOutput, OutputCaptureError> {
    Ok(CapturedOutput {
        bytes: Vec::new(),
        truncated: false,
        complete: false,
    })
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use super::super::captured_output::CapturedOutput;
    use super::super::io_cancellation::IoCancellation;
    use super::super::output_reader::OutputReader;
    use super::super::stdin_writer::StdinWriter;
    use super::CommandIo;
    use super::HELPER_CANCELLATION_CONFIRMATION_TIMEOUT;
    use crate::CommandCleanupFailure;

    fn completed_reader(cancellation: IoCancellation) -> OutputReader {
        OutputReader::new(
            thread::spawn(|| Ok(CapturedOutput::default())),
            cancellation,
        )
    }

    fn completed_writer(cancellation: IoCancellation) -> StdinWriter {
        StdinWriter::new(thread::spawn(|| Ok(())), cancellation)
    }

    fn wait_for_completion(is_finished: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !is_finished() && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(is_finished(), "test helper should finish");
    }

    #[test]
    fn test_finish_helpers_joins_when_all_cancellations_succeed() {
        let (stdout_cancellation, stdout_token) =
            IoCancellation::pair().expect("stdout cancellation should create");
        let stdout_reader = OutputReader::new(
            thread::spawn(move || {
                while !stdout_token.is_cancelled() {
                    thread::yield_now();
                }
                Ok(CapturedOutput::default())
            }),
            stdout_cancellation,
        );
        let (stderr_cancellation, stderr_token) =
            IoCancellation::pair().expect("stderr cancellation should create");
        let stderr_reader = OutputReader::new(
            thread::spawn(move || {
                while !stderr_token.is_cancelled() {
                    thread::yield_now();
                }
                Ok(CapturedOutput::default())
            }),
            stderr_cancellation,
        );

        let failures =
            CommandIo::new(stdout_reader, stderr_reader, None).cancel_and_join("test command");

        assert!(failures.is_empty());
    }

    #[test]
    fn test_finish_helpers_joins_completed_reader_after_cancellation_failure() {
        let reader = completed_reader(IoCancellation::failing("stdout cancellation failed"));
        wait_for_completion(|| reader.is_finished());
        let (stderr_cancellation, _stderr_token) =
            IoCancellation::pair().expect("stderr cancellation should create");
        let stderr = completed_reader(stderr_cancellation);

        let failures = CommandIo::new(reader, stderr, None).cancel_and_join("test command");

        assert_eq!(failures.len(), 1);
        assert!(matches!(
            failures[0],
            CommandCleanupFailure::StdoutCancellation { .. }
        ));
    }

    #[test]
    fn test_finish_helpers_detaches_after_confirmation_timeout() {
        let release = Arc::new(AtomicBool::new(false));
        let worker_release = Arc::clone(&release);
        let blocked = OutputReader::new(
            thread::spawn(move || {
                while !worker_release.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                Ok(CapturedOutput::default())
            }),
            IoCancellation::failing("stdout cancellation failed"),
        );
        let (stderr_cancellation, _stderr_token) =
            IoCancellation::pair().expect("stderr cancellation should create");
        let stderr = completed_reader(stderr_cancellation);
        let started = Instant::now();

        let failures = CommandIo::new(blocked, stderr, None).cancel_and_join("test command");
        release.store(true, Ordering::Release);

        assert!(started.elapsed() >= HELPER_CANCELLATION_CONFIRMATION_TIMEOUT);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(failures.len(), 1);
        assert!(matches!(
            failures[0],
            CommandCleanupFailure::StdoutCancellation { .. }
        ));
    }

    #[test]
    fn test_finish_helpers_orders_all_cancellation_failures_by_helper() {
        let stdout = completed_reader(IoCancellation::failing("stdout cancellation failed"));
        let stderr = completed_reader(IoCancellation::failing("stderr cancellation failed"));
        let stdin = completed_writer(IoCancellation::failing("stdin cancellation failed"));
        wait_for_completion(|| stdout.is_finished());
        wait_for_completion(|| stderr.is_finished());
        wait_for_completion(|| stdin.is_finished());

        let failures = CommandIo::new(stdout, stderr, Some(stdin)).cancel_and_join("test command");

        assert_eq!(failures.len(), 3);
        assert!(matches!(
            failures[0],
            CommandCleanupFailure::StdoutCancellation { .. }
        ));
        assert!(matches!(
            failures[1],
            CommandCleanupFailure::StderrCancellation { .. }
        ));
        assert!(matches!(
            failures[2],
            CommandCleanupFailure::StdinCancellation { .. }
        ));
    }

    #[test]
    fn test_finish_helpers_keeps_output_error_primary_to_cancellation_failure() {
        let reader = OutputReader::new(
            thread::spawn(|| {
                Err(
                    super::super::output_capture_error::OutputCaptureError::Read {
                        source: io::Error::other("stdout read failed"),
                        output: CapturedOutput::default(),
                    },
                )
            }),
            IoCancellation::failing("stdout cancellation failed"),
        );
        wait_for_completion(|| reader.is_finished());
        let (stderr_cancellation, _stderr_token) =
            IoCancellation::pair().expect("stderr cancellation should create");
        let stderr = completed_reader(stderr_cancellation);

        let (output, failures) = CommandIo::new(reader, stderr, None).cancel_and_collect(
            "test command",
            std::process::Command::new("rustc")
                .arg("--version")
                .status()
                .expect("rustc should provide an exit status"),
            || Ok(Duration::ZERO),
        );

        assert!(matches!(
            output
                .expect_err("reader failure should remain primary")
                .reason(),
            crate::CommandErrorReason::ReadOutputFailed { .. }
        ));
        assert_eq!(failures.len(), 1);
        assert!(matches!(
            failures[0],
            CommandCleanupFailure::StdoutCancellation { .. }
        ));
    }
}
