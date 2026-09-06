// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! Coverage-only probes for internal command-runner failure paths.

use std::error::Error;
use std::fs::OpenOptions;
use std::io;
use std::io::Cursor;
use std::io::Write;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use internal::FailingReader;
use internal::FailingWriter;
use qubit_clock::TimeError;
use qubit_clock::TimerUnavailableError;

use super::internal::captured_output::CapturedOutput;
use super::internal::error_mapping::kill_failed;
use super::internal::error_mapping::output_pipe_error;
use super::internal::error_mapping::spawn_failed;
use super::internal::error_mapping::wait_failed;
use super::internal::io_cancellation::IoCancellation;
use super::internal::io_files::__coverage_fail_truncate;
use super::internal::io_files::IoFiles;
#[cfg(unix)]
use super::internal::io_files::ensure_regular_input_handle;
#[cfg(unix)]
use super::internal::io_files::ensure_regular_output_handle;
use super::internal::io_files::normalize_lexically;
#[cfg(unix)]
use super::internal::io_files::open_input_candidate;
#[cfg(unix)]
use super::internal::io_files::open_output_candidate;
use super::internal::io_files::truncate_output;
use super::internal::managed_child_process::__coverage_fail_tree_kill;
use super::internal::managed_child_process::ManagedChildProcess;
use super::internal::output_capture_error::OutputCaptureError;
use super::internal::output_capture_options::OutputCaptureOptions;
use super::internal::output_collector::collect_output;
use super::internal::output_collector::join_output_reader;
use super::internal::output_collector::read_output;
use super::internal::output_collector::read_output_stream;
use super::internal::output_reader::OutputReader;
use super::internal::output_tee::OutputTee;
use super::internal::process_launcher::spawn_child;
use super::internal::starting_command::StartingCommand;
use super::internal::stdin_pipe::__coverage_fail_stdin_thread;
use super::internal::stdin_pipe::join_stdin_writer;
use super::internal::stdin_pipe::map_stdin_thread_result;
use super::internal::stdin_pipe::write_stdin_bytes;
use super::internal::stdin_writer::StdinWriter;
use super::start_output_reader;
use super::take_output_pipe;
use crate::CommandCleanupFailure;
use crate::CommandError;
use crate::CommandErrorReason;
use crate::CommandOutput;
#[cfg(unix)]
use crate::CommandRunner;
use crate::OutputStream;
use crate::command_stdin::CommandStdin;

/// Exercises the public error container and every primary reason formatter.
fn probe_error_container() {
    let io_error = || io::Error::other("coverage error source");
    let reasons = vec![
        CommandErrorReason::SpawnFailed { source: io_error() },
        CommandErrorReason::WaitFailed { source: io_error() },
        CommandErrorReason::CancelledBeforeStart,
        CommandErrorReason::KillFailed {
            timeout: Duration::from_secs(1),
            process_tree_source: io_error(),
            child_source: io_error(),
        },
        CommandErrorReason::ReadOutputFailed {
            stream: OutputStream::Stdout,
            source: io_error(),
        },
        CommandErrorReason::OpenInputFailed {
            path: "input".into(),
            source: io_error(),
        },
        CommandErrorReason::NonRegularInputFile { path: "input".into() },
        CommandErrorReason::OpenOutputFailed {
            stream: OutputStream::Stdout,
            path: "output".into(),
            source: io_error(),
        },
        CommandErrorReason::NonRegularOutputFile {
            stream: OutputStream::Stderr,
            path: "output".into(),
        },
        CommandErrorReason::InputOutputConflict {
            input_path: "input".into(),
            output_stream: OutputStream::Stdout,
            output_path: "output".into(),
        },
        CommandErrorReason::OutputFilesConflict {
            stdout_path: "stdout".into(),
            stderr_path: "stderr".into(),
        },
        CommandErrorReason::InspectIoFileFailed {
            path: "output".into(),
            source: io_error(),
        },
        CommandErrorReason::StartInputThreadFailed { source: io_error() },
        CommandErrorReason::StartOutputThreadFailed {
            stream: OutputStream::Stderr,
            source: io_error(),
        },
        CommandErrorReason::TimeFailed {
            source: TimeError::TimerUnavailable {
                source: TimerUnavailableError::BackendUnavailable {
                    backend: "coverage",
                    source: Box::new(io_error()),
                },
            },
        },
        CommandErrorReason::WriteInputFailed { source: io_error() },
        CommandErrorReason::WriteOutputFailed {
            stream: OutputStream::Stdout,
            path: "output".into(),
            source: io_error(),
        },
        CommandErrorReason::TimedOut {
            timeout: Duration::from_secs(1),
        },
        CommandErrorReason::Cancelled,
        CommandErrorReason::CancelFailed {
            process_tree_source: io_error(),
            child_source: io_error(),
        },
        CommandErrorReason::OutputTruncated,
        CommandErrorReason::UnexpectedExit {
            exit_code: Some(9),
            expected: vec![0],
        },
    ];

    for reason in reasons {
        let error = CommandError::from_reason("probe command", reason, None);
        let _ = error.kind();
        let _ = error.reason();
        let _ = error.output();
        let _ = error.cleanup_failures();
        let _ = error.exit_code();
        let _ = error.is_unexpected_exit();
        let _ = error.process_tree_source();
        let _ = error.child_source();
        let _ = error.source();
        let _ = error.to_string();
        let _ = format!("{error:?}");
    }

    let output = CommandOutput::new(
        ProcessCommand::new("true")
            .status()
            .expect("coverage output process should exit"),
        (b"stdout".to_vec(), false, true),
        (b"stderr".to_vec(), false, true),
        Duration::from_secs(1),
    );
    let unexpected = CommandError::from_reason(
        "probe command",
        CommandErrorReason::UnexpectedExit {
            exit_code: Some(9),
            expected: vec![0],
        },
        Some(Box::new(output)),
    );
    assert_eq!(unexpected.exit_code(), Some(9));
    let _ = unexpected.to_string();

    let cleanup = CommandError::from_reason("probe command", CommandErrorReason::Cancelled, None)
        .with_cleanup_failures([
            CommandCleanupFailure::Wait { source: io_error() },
            CommandCleanupFailure::ProcessTreeTermination { source: io_error() },
            CommandCleanupFailure::ChildTermination { source: io_error() },
            CommandCleanupFailure::Stdin { source: io_error() },
            CommandCleanupFailure::StdoutRead { source: io_error() },
            CommandCleanupFailure::StdoutWrite {
                path: "stdout".into(),
                source: io_error(),
            },
            CommandCleanupFailure::StderrRead { source: io_error() },
            CommandCleanupFailure::StderrWrite {
                path: "stderr".into(),
                source: io_error(),
            },
        ]);
    assert_eq!(cleanup.cleanup_failures().len(), 8);
    assert!(cleanup.process_tree_source().is_some());
    assert!(cleanup.child_source().is_some());
    assert!(cleanup.to_string().contains("8 cleanup failure(s)"));

    let _ = CommandError::from_reason(
        "probe command",
        CommandErrorReason::WriteInputFailed { source: io_error() },
        None,
    )
    .into_cleanup_failure();
    for stream in [OutputStream::Stdout, OutputStream::Stderr] {
        let _ = CommandError::from_reason(
            "probe command",
            CommandErrorReason::ReadOutputFailed {
                stream,
                source: io_error(),
            },
            None,
        )
        .into_cleanup_failure();
        let _ = CommandError::from_reason(
            "probe command",
            CommandErrorReason::WriteOutputFailed {
                stream,
                path: "output".into(),
                source: io_error(),
            },
            None,
        )
        .into_cleanup_failure();
    }
}

mod internal;

/// Runs a deterministic successful process and returns its exit status.
fn status() -> std::process::ExitStatus {
    ProcessCommand::new("rustc")
        .arg("--version")
        .status()
        .expect("coverage process should provide an exit status")
}

/// Builds an output reader whose worker returns a supplied coverage result.
fn output_reader(result: Result<CapturedOutput, OutputCaptureError>) -> OutputReader {
    let (cancellation, token) = IoCancellation::pair().expect("coverage cancellation pair should create");
    let join = thread::spawn(move || {
        let _token = token;
        result
    });
    OutputReader::new(join, cancellation)
}

/// Builds a stdin writer whose worker executes a supplied coverage closure.
fn stdin_writer(write: impl FnOnce() -> io::Result<()> + Send + 'static) -> StdinWriter {
    let (cancellation, token) = IoCancellation::pair().expect("coverage cancellation pair should create");
    StdinWriter::new(
        thread::spawn(move || {
            let _token = token;
            write()
        }),
        cancellation,
    )
}

/// Spawns a deterministic child process for coverage probes.
fn spawn_rustc_child() -> ManagedChildProcess {
    let mut command = ProcessCommand::new("rustc");
    command.arg("--version");
    spawn_child(command, false).expect("coverage process should spawn")
}

/// Verifies that Unix special-file candidates are opened without blocking.
#[cfg(unix)]
fn probe_nonblocking_special_file_open() {
    let fifo_path = std::env::temp_dir().join(format!("qubit-command-coverage-{}-fifo", std::process::id(),));
    let status = ProcessCommand::new("mkfifo")
        .arg(&fifo_path)
        .status()
        .expect("coverage FIFO command should start");
    assert!(status.success(), "coverage FIFO should be created");

    let input = open_input_candidate(&fifo_path).expect("nonblocking FIFO input open should return immediately");
    assert!(matches!(
        ensure_regular_input_handle("command", &fifo_path, &input),
        Err(error) if error.kind() == crate::CommandErrorKind::NonRegularInputFile
    ));

    let output = open_output_candidate(&fifo_path).expect("FIFO output should open while the input handle is live");
    assert!(matches!(
        ensure_regular_output_handle(
            "command",
            OutputStream::Stdout,
            &fifo_path,
            &output,
        ),
        Err(error) if error.kind() == crate::CommandErrorKind::NonRegularOutputFile
    ));

    drop(output);
    drop(input);
    std::fs::remove_file(&fifo_path).expect("coverage FIFO should be removed");
}

/// Executes deterministic coverage probes for internal error and I/O paths.
#[doc(hidden)]
pub fn __coverage_internal() {
    probe_error_container();

    #[cfg(unix)]
    {
        __coverage_fail_tree_kill(true);
        let termination_error = CommandRunner::new(Duration::from_millis(20))
            .run(crate::Command::shell("sleep 1"))
            .expect_err("coverage timeout should retain its primary error");
        __coverage_fail_tree_kill(false);
        assert_eq!(termination_error.kind(), crate::CommandErrorKind::TimedOut);
        assert!(
            termination_error
                .cleanup_failures()
                .iter()
                .any(|failure| { matches!(failure, CommandCleanupFailure::ProcessTreeTermination { .. }) })
        );

        let cancellation = crate::CommandCancellation::new();
        let cancellation_request = cancellation.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            cancellation_request.cancel();
        });
        __coverage_fail_tree_kill(true);
        let cancellation_error = CommandRunner::without_timeout()
            .run_with(
                crate::Command::shell("sleep 1"),
                crate::CommandRunOptions::new().cancellation(cancellation),
            )
            .expect_err("coverage cancellation should retain its primary error");
        __coverage_fail_tree_kill(false);
        canceller.join().expect("coverage cancellation thread should finish");
        assert_eq!(cancellation_error.kind(), crate::CommandErrorKind::Cancelled);
        assert!(
            cancellation_error
                .cleanup_failures()
                .iter()
                .any(|failure| { matches!(failure, CommandCleanupFailure::ProcessTreeTermination { .. }) })
        );
    }

    #[cfg(unix)]
    probe_nonblocking_special_file_open();

    let default_captured = CapturedOutput::default();
    assert!(default_captured.bytes.is_empty());
    assert!(!default_captured.truncated);
    assert!(default_captured.complete);

    let spawn = spawn_failed("spawn", io::Error::other("spawn source"));
    assert_eq!(spawn.kind(), crate::CommandErrorKind::SpawnFailed);
    let wait = wait_failed("wait", io::Error::other("wait source"));
    assert_eq!(wait.kind(), crate::CommandErrorKind::WaitFailed);
    let kill = kill_failed(
        "kill".to_owned(),
        Duration::from_secs(3),
        io::Error::other("kill source"),
        io::Error::other("child kill source"),
    );
    assert_eq!(kill.kind(), crate::CommandErrorKind::KillFailed);
    let pipe = output_pipe_error("pipe", OutputStream::Stdout);
    assert!(pipe.to_string().contains("stdout pipe was not created"));
    assert_eq!(normalize_lexically(Path::new("a/./b/../c")), Path::new("a/c"));

    let stdout_path = std::env::temp_dir().join(format!("qubit-command-coverage-{}-stdout", std::process::id(),));
    let stderr_path = std::env::temp_dir().join(format!("qubit-command-coverage-{}-stderr", std::process::id(),));
    let mut prepare_command = ProcessCommand::new("true");
    let mut io_files = IoFiles::prepare(
        "command",
        CommandStdin::Null,
        Some(&stdout_path),
        Some(&stderr_path),
        &mut prepare_command,
    )
    .expect("coverage output files should prepare");
    io_files
        .commit("command", Some(&stdout_path), Some(&stderr_path), &mut prepare_command)
        .expect("coverage output files should commit");
    std::fs::remove_file(stdout_path).expect("coverage stdout fixture should be removed");
    std::fs::remove_file(stderr_path).expect("coverage stderr fixture should be removed");

    let truncate_path = std::env::temp_dir().join(format!("qubit-command-coverage-{}-truncate", std::process::id(),));
    let truncate_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&truncate_path)
        .expect("coverage truncation fixture should open");
    __coverage_fail_truncate(true);
    let truncate_error = truncate_output(
        "command",
        OutputStream::Stdout,
        Some(&truncate_path),
        Some(&truncate_file),
    )
    .expect_err("coverage truncation failure should be injected");
    __coverage_fail_truncate(false);
    assert_eq!(truncate_error.kind(), crate::CommandErrorKind::OpenOutputFailed);
    drop(truncate_file);
    std::fs::remove_file(truncate_path).expect("coverage fixture should be removed");

    let missing_stdout = take_output_pipe::<std::process::ChildStdout>("command", OutputStream::Stdout, || None)
        .expect_err("coverage missing stdout pipe should be reported");
    assert_eq!(missing_stdout.kind(), crate::CommandErrorKind::ReadOutputFailed);
    let missing_stderr = take_output_pipe::<std::process::ChildStderr>("command", OutputStream::Stderr, || None)
        .expect_err("coverage missing stderr pipe should be reported");
    assert_eq!(missing_stderr.kind(), crate::CommandErrorKind::ReadOutputFailed);
    let stdout_thread = start_output_reader("command", OutputStream::Stdout, || {
        Err(io::Error::other("coverage stdout thread failure"))
    })
    .expect_err("coverage stdout thread failure should be mapped");
    assert_eq!(stdout_thread.kind(), crate::CommandErrorKind::StartOutputThreadFailed);
    let stderr_thread = start_output_reader("command", OutputStream::Stderr, || {
        Err(io::Error::other("coverage stderr thread failure"))
    })
    .expect_err("coverage stderr thread failure should be mapped");
    assert_eq!(stderr_thread.kind(), crate::CommandErrorKind::StartOutputThreadFailed);

    let mut output_child = ProcessCommand::new("rustc")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("coverage output child should spawn");
    let stdout_reader = read_output_stream(
        output_child.stdout.take().expect("coverage stdout pipe should exist"),
        OutputCaptureOptions {
            max_bytes: None,
            tee: None,
        },
    )
    .expect("coverage stdout reader should start");
    let error_reader = read_output_stream(
        output_child.stderr.take().expect("coverage stderr pipe should exist"),
        OutputCaptureOptions {
            max_bytes: None,
            tee: None,
        },
    )
    .expect("coverage stderr reader should start");
    let captured_stdout = join_output_reader(stdout_reader).expect("coverage stdout reader should join");
    let captured_stderr = join_output_reader(error_reader).expect("coverage stderr reader should join");
    output_child.wait().expect("coverage output child should be waitable");
    assert!(captured_stdout.complete);
    assert!(captured_stderr.complete);

    let mut failing_reader = FailingReader::with_prefix(b"partial".to_vec());
    let read_error = read_output(
        &mut failing_reader,
        OutputCaptureOptions {
            max_bytes: None,
            tee: None,
        },
    )
    .expect_err("coverage reader failure should be returned");
    let OutputCaptureError::Read { output, .. } = read_error else {
        panic!("coverage reader failure should retain output");
    };
    assert_eq!(output.bytes, b"partial");
    assert!(!output.complete);

    let tee_input = vec![b'o'; 3 * 8 * 1024];
    let tee_error = read_output(
        &mut Cursor::new(tee_input),
        OutputCaptureOptions {
            max_bytes: Some(2 * 8 * 1024),
            tee: Some(OutputTee {
                writer: Box::new(FailingWriter { fail_write: true }),
                path: "tee-write.log".into(),
            }),
        },
    )
    .expect_err("coverage tee write failure should be returned");
    let OutputCaptureError::Write { output, .. } = tee_error else {
        panic!("coverage tee write failure should retain output");
    };
    assert_eq!(output.bytes.len(), 2 * 8 * 1024);
    assert!(output.truncated);

    let flush_error = read_output(
        &mut Cursor::new(b"output".to_vec()),
        OutputCaptureOptions {
            max_bytes: None,
            tee: Some(OutputTee {
                writer: Box::new(FailingWriter { fail_write: false }),
                path: "tee-flush.log".into(),
            }),
        },
    )
    .expect_err("coverage tee flush failure should be returned");
    assert!(matches!(flush_error, OutputCaptureError::Write { .. }));

    FailingWriter { fail_write: true }
        .flush()
        .expect("coverage write-failure fixture should flush successfully");

    let elapsed_error = collect_output(
        "command",
        status(),
        || {
            Err(TimeError::TimerUnavailable {
                source: TimerUnavailableError::BackendUnavailable {
                    backend: "coverage",
                    source: Box::new(io::Error::other("coverage timer failure")),
                },
            })
        },
        output_reader(Ok(CapturedOutput::default())),
        output_reader(Ok(CapturedOutput::default())),
        None,
    )
    .expect_err("coverage elapsed failure should take precedence");
    assert_eq!(elapsed_error.kind(), crate::CommandErrorKind::TimeFailed);

    let elapsed_with_helper_errors = collect_output(
        "command",
        status(),
        || {
            Err(TimeError::TimerUnavailable {
                source: TimerUnavailableError::BackendUnavailable {
                    backend: "coverage",
                    source: Box::new(io::Error::other("coverage timer failure with helpers")),
                },
            })
        },
        output_reader(Err(OutputCaptureError::Read {
            source: io::Error::other("coverage stdout failure with timer"),
            output: CapturedOutput::default(),
        })),
        output_reader(Err(OutputCaptureError::Write {
            path: "stderr.log".into(),
            source: io::Error::other("coverage stderr failure with timer"),
            output: CapturedOutput::default(),
        })),
        Some(stdin_writer(|| {
            Err(io::Error::other("coverage stdin failure with timer"))
        })),
    )
    .expect_err("coverage elapsed failure should retain helper failures");
    assert_eq!(elapsed_with_helper_errors.kind(), crate::CommandErrorKind::TimeFailed);
    assert_eq!(elapsed_with_helper_errors.cleanup_failures().len(), 3);

    let stdout_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Err(OutputCaptureError::Read {
            source: io::Error::other("coverage stdout failure"),
            output: CapturedOutput {
                bytes: b"partial-stdout".to_vec(),
                truncated: false,
                complete: false,
            },
        })),
        output_reader(Ok(CapturedOutput {
            bytes: b"complete-stderr".to_vec(),
            truncated: false,
            complete: true,
        })),
        None,
    )
    .expect_err("coverage stdout failure should be mapped");
    assert!(matches!(
        stdout_error.reason(),
        crate::CommandErrorReason::ReadOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));
    let stdout_output = stdout_error.output().expect("read failure should retain both streams");
    assert_eq!(stdout_output.stdout(), b"partial-stdout");
    assert_eq!(stdout_output.stderr(), b"complete-stderr");
    assert!(!stdout_output.stdout_complete());
    assert!(stdout_output.stderr_complete());

    let combined_helper_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Err(OutputCaptureError::Read {
            source: io::Error::other("coverage combined stdout failure"),
            output: CapturedOutput::default(),
        })),
        output_reader(Err(OutputCaptureError::Write {
            path: "combined-stderr.log".into(),
            source: io::Error::other("coverage combined stderr failure"),
            output: CapturedOutput::default(),
        })),
        Some(stdin_writer(|| {
            Err(io::Error::other("coverage combined stdin failure"))
        })),
    )
    .expect_err("coverage stdout failure should retain other helper failures");
    assert!(matches!(
        combined_helper_error.reason(),
        crate::CommandErrorReason::ReadOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));
    assert_eq!(combined_helper_error.cleanup_failures().len(), 2);
    assert!(
        combined_helper_error
            .cleanup_failures()
            .iter()
            .any(|failure| { matches!(failure, CommandCleanupFailure::StderrWrite { .. }) })
    );
    assert!(
        combined_helper_error
            .cleanup_failures()
            .iter()
            .any(|failure| { matches!(failure, CommandCleanupFailure::Stdin { .. }) })
    );

    let stderr_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Ok(CapturedOutput {
            bytes: b"out".to_vec(),
            truncated: false,
            complete: true,
        })),
        output_reader(Err(OutputCaptureError::Read {
            source: io::Error::other("coverage stderr failure"),
            output: CapturedOutput {
                bytes: b"partial-stderr".to_vec(),
                truncated: false,
                complete: false,
            },
        })),
        None,
    )
    .expect_err("coverage stderr failure should be mapped");
    assert!(matches!(
        stderr_error.reason(),
        crate::CommandErrorReason::ReadOutputFailed {
            stream: OutputStream::Stderr,
            ..
        }
    ));

    let stdout_tee_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Err(OutputCaptureError::Write {
            path: "stdout.log".into(),
            source: io::Error::other("coverage stdout tee failure"),
            output: CapturedOutput {
                bytes: b"stdout".to_vec(),
                truncated: false,
                complete: true,
            },
        })),
        output_reader(Ok(CapturedOutput {
            bytes: b"stderr".to_vec(),
            truncated: false,
            complete: true,
        })),
        None,
    )
    .expect_err("coverage stdout tee failure should be mapped");
    assert!(matches!(
        stdout_tee_error.reason(),
        crate::CommandErrorReason::WriteOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));

    let stderr_tee_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Ok(CapturedOutput {
            bytes: b"stdout".to_vec(),
            truncated: false,
            complete: true,
        })),
        output_reader(Err(OutputCaptureError::Write {
            path: "stderr.log".into(),
            source: io::Error::other("coverage stderr tee failure"),
            output: CapturedOutput {
                bytes: b"stderr".to_vec(),
                truncated: false,
                complete: true,
            },
        })),
        None,
    )
    .expect_err("coverage stderr tee failure should be mapped");
    assert!(matches!(
        stderr_tee_error.reason(),
        crate::CommandErrorReason::WriteOutputFailed {
            stream: OutputStream::Stderr,
            ..
        }
    ));

    let stdin_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Ok(CapturedOutput {
            bytes: b"out".to_vec(),
            truncated: false,
            complete: true,
        })),
        output_reader(Ok(CapturedOutput {
            bytes: b"err".to_vec(),
            truncated: false,
            complete: true,
        })),
        Some(stdin_writer(|| -> io::Result<()> {
            panic!("coverage stdin writer failure");
        })),
    )
    .expect_err("coverage stdin failure should be returned");
    assert_eq!(stdin_error.kind(), crate::CommandErrorKind::WriteInputFailed);
    let stdin_output = stdin_error
        .output()
        .expect("stdin failure should retain completed output");
    assert_eq!(stdin_output.stdout(), b"out");
    assert_eq!(stdin_output.stderr(), b"err");

    let output = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Ok(CapturedOutput {
            bytes: b"out".to_vec(),
            truncated: true,
            complete: true,
        })),
        output_reader(Ok(CapturedOutput {
            bytes: b"err".to_vec(),
            truncated: false,
            complete: true,
        })),
        None,
    )
    .expect("coverage helper results should produce output");
    assert_eq!(output.stdout(), b"out");
    assert_eq!(output.stderr(), b"err");
    assert!(output.stdout_truncated());

    let joined = join_output_reader(output_reader(Ok(CapturedOutput {
        bytes: b"ok".to_vec(),
        truncated: false,
        complete: true,
    })))
    .expect("successful coverage reader should join");
    assert_eq!(joined.bytes, b"ok");
    let joined_error = join_output_reader(output_reader(Err(OutputCaptureError::Read {
        source: io::Error::other("coverage read failure"),
        output: CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
            complete: false,
        },
    })))
    .expect_err("coverage reader error should be preserved");
    assert!(matches!(joined_error, OutputCaptureError::Read { .. }));
    let joined_panic = join_output_reader(output_reader(Err(OutputCaptureError::Read {
        source: io::Error::other("coverage reader panic"),
        output: CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
            complete: false,
        },
    })))
    .expect_err("coverage reader panic should map to a read error");
    assert!(matches!(joined_panic, OutputCaptureError::Read { .. }));

    let mut no_pipe = ProcessCommand::new("rustc")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("coverage child should spawn");
    let missing_stdin = write_stdin_bytes("command", &mut no_pipe, Some(b"input".to_vec()))
        .expect_err("coverage missing stdin pipe should be reported");
    assert_eq!(missing_stdin.kind(), crate::CommandErrorKind::WriteInputFailed);
    no_pipe.wait().expect("coverage child should be waitable");

    let mut piped = ProcessCommand::new("rustc")
        .arg("--version")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("coverage child should spawn with stdin");
    let writer = write_stdin_bytes("command", &mut piped, Some(b"input".to_vec()))
        .expect("coverage stdin writer should start")
        .expect("coverage stdin writer should exist");
    join_stdin_writer("command", Some(writer)).expect("coverage stdin writer should finish");
    piped.wait().expect("coverage child should be waitable");
    assert!(
        write_stdin_bytes("command", &mut piped, None)
            .expect("coverage absent stdin should be accepted")
            .is_none()
    );

    let mut injected = ProcessCommand::new("rustc")
        .arg("--version")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("coverage child should spawn for injected failure");
    __coverage_fail_stdin_thread(true);
    let injected_error = write_stdin_bytes("command", &mut injected, Some(b"input".to_vec()))
        .expect_err("coverage stdin thread failure should be injected");
    __coverage_fail_stdin_thread(false);
    assert_eq!(injected_error.kind(), crate::CommandErrorKind::StartInputThreadFailed);
    injected.kill().expect("injected child should be stoppable");
    injected.wait().expect("injected child should be waitable");
    let mapped_stdin_error = map_stdin_thread_result("command", Err(io::Error::other("coverage thread start failure")))
        .expect_err("coverage thread start mapping should preserve the error");
    assert_eq!(
        mapped_stdin_error.kind(),
        crate::CommandErrorKind::StartInputThreadFailed
    );

    join_stdin_writer(
        "command",
        Some(stdin_writer(|| {
            Err::<(), io::Error>(io::Error::from(io::ErrorKind::BrokenPipe))
        })),
    )
    .expect("coverage broken pipe should be accepted");
    let stdin_write_error = join_stdin_writer(
        "command",
        Some(stdin_writer(|| {
            Err::<(), io::Error>(io::Error::other("coverage stdin failure"))
        })),
    )
    .expect_err("coverage stdin failure should be mapped");
    assert_eq!(stdin_write_error.kind(), crate::CommandErrorKind::WriteInputFailed);
    let stdin_panic = join_stdin_writer(
        "command",
        Some(stdin_writer(|| -> io::Result<()> {
            panic!("coverage stdin panic");
        })),
    )
    .expect_err("coverage stdin panic should be mapped");
    assert_eq!(stdin_panic.kind(), crate::CommandErrorKind::WriteInputFailed);
    join_stdin_writer("command", None).expect("coverage absent writer should be accepted");

    let child = spawn_rustc_child();
    drop(StartingCommand::new("coverage-cleanup", child));

    let mut exited_child = spawn_rustc_child();
    exited_child.wait().expect("coverage child should exit before cleanup");
    drop(StartingCommand::new("coverage-exited-cleanup", exited_child));
}
