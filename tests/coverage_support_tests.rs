/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Regression tests for defensive command-runner behavior.

#[path = "../src/command_error.rs"]
mod command_error;
#[path = "../src/command_output.rs"]
mod command_output;
#[path = "../src/output_stream.rs"]
mod output_stream;
mod command_runner {
    pub(crate) mod captured_output {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/captured_output.rs"
        ));
    }
    pub(crate) mod command_io {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/command_io.rs"
        ));
    }
    pub(crate) mod error_mapping {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/error_mapping.rs"
        ));
    }
    pub(crate) mod finished_command {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/finished_command.rs"
        ));
    }
    pub(crate) mod managed_child_process {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/managed_child_process.rs"
        ));
    }
    pub(crate) mod output_capture_error {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/output_capture_error.rs"
        ));
    }
    pub(crate) mod output_capture_options {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/output_capture_options.rs"
        ));
    }
    pub(crate) mod output_collector {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/output_collector.rs"
        ));
    }
    pub(crate) mod output_reader {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/output_reader.rs"
        ));
    }
    pub(crate) mod output_tee {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/output_tee.rs"
        ));
    }
    pub(crate) mod running_command {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/running_command.rs"
        ));
    }
    pub(crate) mod stdin_pipe {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/stdin_pipe.rs"
        ));
    }
    pub(crate) mod stdin_writer {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/stdin_writer.rs"
        ));
    }
    pub(crate) mod wait_policy {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command_runner/wait_policy.rs"
        ));
    }
}

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::{
    io::{
        self,
        Read,
        Write,
    },
    panic,
    path::PathBuf,
    process::{
        ChildStderr,
        ChildStdin,
        ChildStdout,
        Command as ProcessCommand,
        ExitStatus,
        Stdio,
    },
    thread,
    time::Duration,
};

pub use command_error::CommandError;
pub use command_output::CommandOutput;
use command_runner::{
    captured_output::CapturedOutput,
    command_io::CommandIo,
    error_mapping::{
        kill_failed,
        output_pipe_error,
        spawn_failed,
        wait_failed,
    },
    managed_child_process::ManagedChildProcess,
    output_capture_error::OutputCaptureError,
    output_capture_options::OutputCaptureOptions,
    output_collector::{
        collect_output,
        join_output_reader,
        read_output,
        read_output_stream,
    },
    output_reader::OutputReader,
    output_tee::OutputTee,
    running_command::RunningCommand,
    stdin_pipe::{
        join_stdin_writer,
        write_stdin_bytes,
    },
    stdin_writer::StdinWriter,
};
pub use output_stream::OutputStream;
use process_wrap::std::ChildWrapper;

use command_runner::finished_command::FinishedCommand;

/// Reader that always fails when read.
struct FailingReader;

impl Read for FailingReader {
    /// Reports a deterministic read failure.
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("read failed"))
    }
}

/// Writer that fails on every write but flushes successfully.
struct FailingWrite;

impl Write for FailingWrite {
    /// Reports a deterministic write failure.
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("write failed"))
    }

    /// Reports a successful flush.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Writer that accepts writes but fails when flushed.
struct FailingFlush;

impl Write for FailingFlush {
    /// Accepts all supplied bytes.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        Ok(buffer.len())
    }

    /// Reports a deterministic flush failure.
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("flush failed"))
    }
}

/// Synthetic child used by tests to exercise process-control failures.
#[derive(Debug, Default)]
struct TestChild {
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    try_wait_error: Option<&'static str>,
    clear_try_wait_error_after_first: bool,
    pending_checks: usize,
    pending: bool,
    exited_after_kill_attempt: bool,
    kill_attempted: bool,
    kill_error: Option<&'static str>,
    wait_error: Option<&'static str>,
}

impl ChildWrapper for TestChild {
    /// Returns this synthetic child as the innermost wrapper.
    fn inner(&self) -> &dyn ChildWrapper {
        self
    }

    /// Returns this synthetic child as the innermost mutable wrapper.
    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        self
    }

    /// Consumes this synthetic child.
    fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
        self
    }

    /// Returns the synthetic stdin pipe.
    fn stdin(&mut self) -> &mut Option<ChildStdin> {
        &mut self.stdin
    }

    /// Returns the synthetic stdout pipe.
    fn stdout(&mut self) -> &mut Option<ChildStdout> {
        &mut self.stdout
    }

    /// Returns the synthetic stderr pipe.
    fn stderr(&mut self) -> &mut Option<ChildStderr> {
        &mut self.stderr
    }

    /// Returns a dummy process identifier.
    fn id(&self) -> u32 {
        0
    }

    /// Reports configured pending, success, or wait-error state.
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(message) = self.try_wait_error {
            if self.clear_try_wait_error_after_first {
                self.try_wait_error = None;
            }
            Err(io::Error::other(message))
        } else if self.pending_checks > 0 {
            self.pending_checks -= 1;
            Ok(None)
        } else if self.pending && !(self.kill_attempted && self.exited_after_kill_attempt) {
            Ok(None)
        } else {
            Ok(Some(success_status()))
        }
    }

    /// Reports configured wait result.
    fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(message) = self.wait_error {
            Err(io::Error::other(message))
        } else {
            Ok(success_status())
        }
    }

    /// Records the kill attempt and reports configured kill result.
    fn start_kill(&mut self) -> io::Result<()> {
        self.kill_attempted = true;
        if let Some(message) = self.kill_error {
            Err(io::Error::other(message))
        } else {
            Ok(())
        }
    }
}

/// Creates a successful exit status for synthetic children.
fn success_status() -> ExitStatus {
    ExitStatus::from_raw(0)
}

/// Creates an output reader that succeeds with the supplied bytes.
fn reader_ok(bytes: Vec<u8>) -> OutputReader {
    thread::spawn(move || {
        Ok(CapturedOutput {
            bytes,
            truncated: false,
        })
    })
}

/// Creates an output reader that reports a read failure.
fn reader_read_error(message: &'static str) -> OutputReader {
    thread::spawn(move || Err(OutputCaptureError::Read(io::Error::other(message))))
}

/// Creates an output reader that reports a tee write failure.
fn reader_write_error(message: &'static str) -> OutputReader {
    thread::spawn(move || {
        Err(OutputCaptureError::Write {
            path: PathBuf::from("reader-output.txt"),
            source: io::Error::other(message),
        })
    })
}

/// Creates an output reader that panics when joined.
fn reader_panic() -> OutputReader {
    thread::spawn(move || -> Result<CapturedOutput, OutputCaptureError> {
        panic!("output reader panic");
    })
}

/// Creates a stdin writer that reports an arbitrary write failure.
fn stdin_writer_error(message: &'static str) -> StdinWriter {
    Some(thread::spawn(move || Err(io::Error::other(message))))
}

/// Creates a stdin writer that reports a broken pipe.
fn stdin_writer_broken_pipe() -> StdinWriter {
    Some(thread::spawn(move || {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "stdin broken pipe",
        ))
    }))
}

/// Creates a stdin writer that panics when joined.
fn stdin_writer_panic() -> StdinWriter {
    Some(thread::spawn(move || -> io::Result<()> {
        panic!("stdin writer panic");
    }))
}

/// Creates a command I/O bundle with empty output readers and no stdin writer.
fn empty_command_io() -> CommandIo {
    CommandIo::new(reader_ok(Vec::new()), reader_ok(Vec::new()), None)
}

/// Creates a command I/O bundle with a failing stdout reader.
fn failing_stdout_command_io() -> CommandIo {
    CommandIo::new(
        reader_read_error("stdout collection failed"),
        reader_ok(Vec::new()),
        None,
    )
}

/// Creates a command I/O bundle with a failing stdin writer.
fn failing_stdin_command_io() -> CommandIo {
    CommandIo::new(
        reader_ok(Vec::new()),
        reader_ok(Vec::new()),
        stdin_writer_error("stdin collection failed"),
    )
}

/// Boxes a synthetic child for use by [`RunningCommand`].
fn boxed_child(child: TestChild) -> ManagedChildProcess {
    Box::new(child)
}

/// Creates an empty stdout pipe from a short-lived real process.
fn empty_stdout() -> ChildStdout {
    let mut child = empty_process_command()
        .stdout(Stdio::piped())
        .spawn()
        .expect("empty stdout child should spawn");
    let stdout = child.stdout.take().expect("stdout should be piped");
    child.wait().expect("empty stdout child should finish");
    stdout
}

/// Creates an empty stderr pipe from a short-lived real process.
fn empty_stderr() -> ChildStderr {
    let mut child = empty_process_command()
        .stderr(Stdio::piped())
        .spawn()
        .expect("empty stderr child should spawn");
    let stderr = child.stderr.take().expect("stderr should be piped");
    child.wait().expect("empty stderr child should finish");
    stderr
}

/// Creates a portable no-op process command.
fn empty_process_command() -> ProcessCommand {
    #[cfg(not(windows))]
    {
        let mut command = ProcessCommand::new("sh");
        command.arg("-c").arg(":");
        command
    }
    #[cfg(windows)]
    {
        let mut command = ProcessCommand::new("cmd");
        command.arg("/C").arg("exit /B 0");
        command
    }
}

/// Builds capture options using an arbitrary writer.
fn capture_with_writer(writer: Box<dyn Write + Send>, path: &str) -> OutputCaptureOptions {
    OutputCaptureOptions {
        max_bytes: None,
        tee: Some(OutputTee {
            writer,
            path: PathBuf::from(path),
        }),
    }
}

/// Extracts the error from a command-runner result without requiring success
/// values to implement [`Debug`](std::fmt::Debug).
fn expect_command_error(
    result: Result<FinishedCommand, CommandError>,
    message: &str,
) -> CommandError {
    match result {
        Ok(_) => panic!("{message}"),
        Err(error) => error,
    }
}

#[test]
fn test_error_mapping_builds_all_error_variants() {
    assert!(matches!(
        spawn_failed("spawn", io::Error::other("spawn failed")),
        CommandError::SpawnFailed { .. }
    ));
    assert!(matches!(
        wait_failed("wait", io::Error::other("wait failed")),
        CommandError::WaitFailed { .. }
    ));
    assert!(matches!(
        kill_failed(
            "kill".to_owned(),
            Duration::from_millis(1),
            io::Error::other("kill failed"),
        ),
        CommandError::KillFailed { .. }
    ));
    assert!(matches!(
        output_pipe_error("pipe", OutputStream::Stdout),
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));
    assert!(matches!(
        output_pipe_error("pipe", OutputStream::Stderr),
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stderr,
            ..
        }
    ));
}

#[test]
fn test_read_output_reports_read_write_and_flush_failures() {
    let read_error = read_output(
        &mut FailingReader,
        OutputCaptureOptions::new(None, None, None),
    )
    .expect_err("failing reader should report read error");
    assert!(matches!(read_error, OutputCaptureError::Read(_)));

    let write_error = read_output(
        &mut io::Cursor::new(b"write".to_vec()),
        capture_with_writer(Box::new(FailingWrite), "stdout.txt"),
    )
    .expect_err("failing writer should report write error");
    assert!(matches!(write_error, OutputCaptureError::Write { .. }));

    let flush_error = read_output(
        &mut io::Cursor::new(b"flush".to_vec()),
        capture_with_writer(Box::new(FailingFlush), "stderr.txt"),
    )
    .expect_err("failing flush should report write error");
    assert!(matches!(flush_error, OutputCaptureError::Write { .. }));
}

#[test]
fn test_read_output_stream_drains_reader_thread() {
    let reader = read_output_stream(
        Box::new(io::Cursor::new(b"threaded".to_vec())),
        OutputCaptureOptions::new(None, None, None),
    );
    let output = reader
        .join()
        .expect("reader thread should not panic")
        .expect("reader should succeed");

    assert_eq!(output.bytes, b"threaded");
    assert!(!output.truncated);
}

#[test]
fn test_read_output_covers_successful_limited_and_tee_paths() {
    let output = read_output(
        &mut io::Cursor::new(b"abc".to_vec()),
        OutputCaptureOptions::new(Some(8), None, None),
    )
    .expect("limited read should succeed without truncation");
    assert_eq!(output.bytes, b"abc");
    assert!(!output.truncated);

    let output = read_output(
        &mut io::Cursor::new(b"abcdef".to_vec()),
        OutputCaptureOptions::new(Some(3), None, None),
    )
    .expect("limited read should report truncation");
    assert_eq!(output.bytes, b"abc");
    assert!(output.truncated);

    let output = read_output(
        &mut io::Cursor::new(b"tee".to_vec()),
        capture_with_writer(Box::new(io::sink()), "sink.txt"),
    )
    .expect("successful tee writer should flush cleanly");
    assert_eq!(output.bytes, b"tee");
    assert!(!output.truncated);
}

#[test]
fn test_collect_output_maps_reader_and_stdin_errors() {
    let error = collect_output(
        "collect-stdout",
        success_status(),
        Duration::ZERO,
        reader_read_error("collect stdout failed"),
        reader_ok(Vec::new()),
        None,
    )
    .expect_err("stdout reader error should be mapped");
    assert!(matches!(
        error,
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));

    let error = collect_output(
        "collect-stderr",
        success_status(),
        Duration::ZERO,
        reader_ok(Vec::new()),
        reader_read_error("collect stderr failed"),
        None,
    )
    .expect_err("stderr reader error should be mapped");
    assert!(matches!(
        error,
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stderr,
            ..
        }
    ));

    let error = collect_output(
        "collect-stdin",
        success_status(),
        Duration::ZERO,
        reader_ok(Vec::new()),
        reader_ok(Vec::new()),
        stdin_writer_error("collect stdin failed"),
    )
    .expect_err("stdin writer error should be mapped");
    assert!(matches!(error, CommandError::WriteInputFailed { .. }));
}

#[test]
fn test_join_output_reader_maps_write_error_and_panic() {
    let error = join_output_reader(
        "writer",
        OutputStream::Stdout,
        reader_write_error("reader write failed"),
    )
    .expect_err("writer error should be mapped");
    assert!(matches!(error, CommandError::WriteOutputFailed { .. }));

    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let panic_error = join_output_reader("panic", OutputStream::Stderr, reader_panic())
        .expect_err("reader panic should be mapped");
    panic::set_hook(previous_hook);

    assert!(matches!(
        panic_error,
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stderr,
            ..
        }
    ));
}

#[test]
fn test_stdin_pipe_maps_missing_pipe_write_error_and_panic() {
    let mut missing_stdin_child = TestChild::default();
    let error = write_stdin_bytes(
        "missing-stdin",
        &mut missing_stdin_child,
        Some(b"x".to_vec()),
    )
    .expect_err("missing stdin pipe should be reported");
    assert!(matches!(error, CommandError::WriteInputFailed { .. }));

    let mut no_input_child = TestChild::default();
    let writer = write_stdin_bytes("no-stdin", &mut no_input_child, None)
        .expect("missing configured bytes should not require stdin");
    assert!(writer.is_none());

    let error = join_stdin_writer("stdin-write", stdin_writer_error("stdin write failed"))
        .expect_err("stdin write error should be mapped");
    assert!(matches!(error, CommandError::WriteInputFailed { .. }));

    join_stdin_writer("stdin-broken-pipe", stdin_writer_broken_pipe())
        .expect("broken pipe should be ignored after process exit");

    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let panic_error = join_stdin_writer("stdin-panic", stdin_writer_panic())
        .expect_err("stdin writer panic should be mapped");
    panic::set_hook(previous_hook);

    assert!(matches!(panic_error, CommandError::WriteInputFailed { .. }));
}

#[test]
#[cfg(not(windows))]
fn test_stdin_pipe_writes_to_real_child_stdin() {
    let mut child = ProcessCommand::new("sh")
        .arg("-c")
        .arg("cat >/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stdin test child should spawn");
    let writer = write_stdin_bytes("stdin-success", &mut child, Some(b"input".to_vec()))
        .expect("stdin writer should start");

    join_stdin_writer("stdin-success", writer).expect("stdin writer should finish");
    child.wait().expect("stdin test child should exit");
}

#[test]
fn test_running_command_maps_wait_error_with_exited_cleanup() {
    let child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        try_wait_error: Some("try wait failed"),
        clear_try_wait_error_after_first: true,
        pending: true,
        exited_after_kill_attempt: true,
        ..TestChild::default()
    };
    let error = expect_command_error(
        RunningCommand::new(
            "wait-error".to_owned(),
            boxed_child(child),
            empty_command_io(),
        )
        .wait_for_completion(None),
        "try-wait failure should be reported",
    );

    assert!(matches!(error, CommandError::WaitFailed { .. }));
}

#[test]
fn test_running_command_completes_after_pending_poll() {
    let child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending_checks: 1,
        ..TestChild::default()
    };
    let finished = RunningCommand::new(
        "pending-then-success".to_owned(),
        boxed_child(child),
        empty_command_io(),
    )
    .wait_for_completion(Some(Duration::from_millis(100)))
    .expect("child should complete after one pending poll");

    assert_eq!(finished.command_text, "pending-then-success");
}

#[test]
fn test_running_command_completes_after_pending_poll_without_timeout() {
    let child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending_checks: 1,
        ..TestChild::default()
    };
    let finished = RunningCommand::new(
        "pending-without-timeout".to_owned(),
        boxed_child(child),
        empty_command_io(),
    )
    .wait_for_completion(None)
    .expect("child should complete after one pending poll");

    assert_eq!(finished.command_text, "pending-without-timeout");
}

#[test]
fn test_running_command_propagates_collection_error_on_normal_exit() {
    let child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        ..TestChild::default()
    };
    let error = expect_command_error(
        RunningCommand::new(
            "normal-collection-error".to_owned(),
            boxed_child(child),
            failing_stdout_command_io(),
        )
        .wait_for_completion(None),
        "output collection error should be reported",
    );

    assert!(matches!(
        error,
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));
}

#[test]
fn test_running_command_preserves_wait_error_when_child_stays_pending() {
    let child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        try_wait_error: Some("try wait failed"),
        clear_try_wait_error_after_first: true,
        pending: true,
        ..TestChild::default()
    };
    let error = expect_command_error(
        RunningCommand::new(
            "pending-wait-error".to_owned(),
            boxed_child(child),
            empty_command_io(),
        )
        .wait_for_completion(None),
        "try-wait failure should be reported",
    );

    assert!(matches!(error, CommandError::WaitFailed { .. }));
}

#[test]
fn test_running_command_propagates_collection_error_after_timeout() {
    let child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        exited_after_kill_attempt: true,
        ..TestChild::default()
    };
    let error = expect_command_error(
        RunningCommand::new(
            "timeout-collection-error".to_owned(),
            boxed_child(child),
            failing_stdin_command_io(),
        )
        .wait_for_completion(Some(Duration::ZERO)),
        "timeout collection error should be reported before timeout output",
    );

    assert!(matches!(error, CommandError::WriteInputFailed { .. }));
}

#[test]
fn test_running_command_returns_timeout_after_successful_kill_and_wait() {
    let child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        exited_after_kill_attempt: true,
        ..TestChild::default()
    };
    let error = expect_command_error(
        RunningCommand::new(
            "timeout-success".to_owned(),
            boxed_child(child),
            empty_command_io(),
        )
        .wait_for_completion(Some(Duration::ZERO)),
        "successful timeout cleanup should report timeout",
    );

    assert!(matches!(error, CommandError::TimedOut { .. }));
}

#[test]
fn test_running_command_maps_timeout_kill_and_wait_errors() {
    let kill_error_child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        kill_error: Some("kill failed"),
        exited_after_kill_attempt: true,
        ..TestChild::default()
    };
    let error = expect_command_error(
        RunningCommand::new(
            "kill-error".to_owned(),
            boxed_child(kill_error_child),
            empty_command_io(),
        )
        .wait_for_completion(Some(Duration::ZERO)),
        "kill failure should be reported",
    );
    assert!(matches!(error, CommandError::KillFailed { .. }));

    let wait_error_child = TestChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        wait_error: Some("wait after kill failed"),
        exited_after_kill_attempt: true,
        ..TestChild::default()
    };
    let error = expect_command_error(
        RunningCommand::new(
            "wait-after-kill-error".to_owned(),
            boxed_child(wait_error_child),
            empty_command_io(),
        )
        .wait_for_completion(Some(Duration::ZERO)),
        "wait-after-kill failure should be reported",
    );

    assert!(matches!(error, CommandError::WaitFailed { .. }));
}
