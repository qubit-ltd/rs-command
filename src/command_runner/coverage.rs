// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! Coverage-only probes for internal command-runner failure paths.

use std::{
    fs::OpenOptions,
    io::{
        self,
        Cursor,
        Write,
    },
    path::Path,
    process::Command as ProcessCommand,
    thread,
    time::Duration,
};

use super::internal::{
    captured_output::CapturedOutput,
    error_mapping::{
        kill_failed,
        output_pipe_error,
        spawn_failed,
        wait_failed,
    },
    io_files::{
        __coverage_fail_truncate,
        IoFiles,
        normalize_lexically,
        truncate_output,
    },
    output_capture_error::OutputCaptureError,
    output_capture_options::OutputCaptureOptions,
    output_collector::{
        collect_output,
        join_output_reader,
        read_output,
    },
    output_reader::OutputReader,
    output_tee::OutputTee,
    starting_command::StartingCommand,
    stdin_pipe::{
        __coverage_fail_stdin_thread,
        join_stdin_writer,
        map_stdin_thread_result,
        write_stdin_bytes,
    },
};
use super::{
    start_output_reader,
    take_output_pipe,
};
use crate::command_stdin::CommandStdin;
use crate::{
    CommandError,
    OutputStream,
};
use internal::{
    FailingReader,
    FailingWriter,
};

mod internal;

fn status() -> std::process::ExitStatus {
    ProcessCommand::new("rustc")
        .arg("--version")
        .status()
        .expect("coverage process should provide an exit status")
}

fn output_reader(
    result: Result<CapturedOutput, OutputCaptureError>,
) -> OutputReader {
    thread::spawn(move || result)
}

fn spawn_rustc_child() -> Box<dyn process_wrap::std::ChildWrapper> {
    let mut command = ProcessCommand::new("rustc");
    command.arg("--version");
    super::internal::process_launcher::spawn_child(command, false)
        .expect("coverage process should spawn")
}

/// Executes deterministic coverage probes for internal error and I/O paths.
#[doc(hidden)]
pub fn __coverage_internal() {
    let spawn = spawn_failed("spawn", io::Error::other("spawn source"));
    assert!(matches!(spawn, CommandError::SpawnFailed { .. }));
    let wait = wait_failed("wait", io::Error::other("wait source"));
    assert!(matches!(wait, CommandError::WaitFailed { .. }));
    let kill = kill_failed(
        "kill".to_owned(),
        Duration::from_secs(3),
        io::Error::other("kill source"),
    );
    assert!(matches!(kill, CommandError::KillFailed { .. }));
    let pipe = output_pipe_error("pipe", OutputStream::Stdout);
    assert!(pipe.to_string().contains("stdout pipe was not created"));
    assert_eq!(
        normalize_lexically(Path::new("a/./b/../c")),
        Path::new("a/c")
    );

    let stdout_path = std::env::temp_dir().join(format!(
        "qubit-command-coverage-{}-stdout",
        std::process::id(),
    ));
    let stderr_path = std::env::temp_dir().join(format!(
        "qubit-command-coverage-{}-stderr",
        std::process::id(),
    ));
    let mut prepare_command = ProcessCommand::new("true");
    IoFiles::prepare(
        "command",
        CommandStdin::Null,
        Some(&stdout_path),
        Some(&stderr_path),
        &mut prepare_command,
    )
    .expect("coverage output files should prepare");
    std::fs::remove_file(stdout_path)
        .expect("coverage stdout fixture should be removed");
    std::fs::remove_file(stderr_path)
        .expect("coverage stderr fixture should be removed");

    let truncate_path = std::env::temp_dir().join(format!(
        "qubit-command-coverage-{}-truncate",
        std::process::id(),
    ));
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
    assert!(matches!(
        truncate_error,
        CommandError::OpenOutputFailed { .. }
    ));
    drop(truncate_file);
    std::fs::remove_file(truncate_path)
        .expect("coverage fixture should be removed");

    let missing_stdout = take_output_pipe::<std::process::ChildStdout>(
        "command",
        OutputStream::Stdout,
        || None,
    )
    .expect_err("coverage missing stdout pipe should be reported");
    assert!(matches!(
        missing_stdout,
        CommandError::ReadOutputFailed { .. }
    ));
    let missing_stderr = take_output_pipe::<std::process::ChildStderr>(
        "command",
        OutputStream::Stderr,
        || None,
    )
    .expect_err("coverage missing stderr pipe should be reported");
    assert!(matches!(
        missing_stderr,
        CommandError::ReadOutputFailed { .. }
    ));
    let stdout_thread =
        start_output_reader("command", OutputStream::Stdout, || {
            Err(io::Error::other("coverage stdout thread failure"))
        })
        .expect_err("coverage stdout thread failure should be mapped");
    assert!(matches!(
        stdout_thread,
        CommandError::StartOutputThreadFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));
    let stderr_thread =
        start_output_reader("command", OutputStream::Stderr, || {
            Err(io::Error::other("coverage stderr thread failure"))
        })
        .expect_err("coverage stderr thread failure should be mapped");
    assert!(matches!(
        stderr_thread,
        CommandError::StartOutputThreadFailed {
            stream: OutputStream::Stderr,
            ..
        }
    ));

    let read_error = read_output(
        &mut FailingReader,
        OutputCaptureOptions {
            max_bytes: None,
            tee: None,
        },
    )
    .expect_err("coverage reader failure should be returned");
    assert!(matches!(read_error, OutputCaptureError::Read(_)));

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
            Err(qubit_clock::TimeError::TimerUnavailable {
                source:
                    qubit_clock::TimerUnavailableError::BackendUnavailable {
                        backend: "coverage",
                        source: Box::new(io::Error::other(
                            "coverage timer failure",
                        )),
                    },
            })
        },
        output_reader(Ok(CapturedOutput::default())),
        output_reader(Ok(CapturedOutput::default())),
        None,
    )
    .expect_err("coverage elapsed failure should take precedence");
    assert!(matches!(elapsed_error, CommandError::TimeFailed { .. }));

    let stdout_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Err(OutputCaptureError::Read(io::Error::other(
            "coverage stdout failure",
        )))),
        output_reader(Ok(CapturedOutput::default())),
        None,
    )
    .expect_err("coverage stdout failure should be mapped");
    assert!(matches!(
        stdout_error,
        CommandError::ReadOutputFailed {
            stream: OutputStream::Stdout,
            ..
        }
    ));

    let stderr_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Ok(CapturedOutput {
            bytes: b"out".to_vec(),
            truncated: false,
        })),
        output_reader(Err(OutputCaptureError::Read(io::Error::other(
            "coverage stderr failure",
        )))),
        None,
    )
    .expect_err("coverage stderr failure should be mapped");
    assert!(matches!(
        stderr_error,
        CommandError::ReadOutputFailed {
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
            },
        })),
        output_reader(Ok(CapturedOutput {
            bytes: b"stderr".to_vec(),
            truncated: false,
        })),
        None,
    )
    .expect_err("coverage stdout tee failure should be mapped");
    assert!(matches!(
        stdout_tee_error,
        CommandError::WriteOutputFailed {
            stream: OutputStream::Stdout,
            output: Some(_),
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
        })),
        output_reader(Err(OutputCaptureError::Write {
            path: "stderr.log".into(),
            source: io::Error::other("coverage stderr tee failure"),
            output: CapturedOutput {
                bytes: b"stderr".to_vec(),
                truncated: false,
            },
        })),
        None,
    )
    .expect_err("coverage stderr tee failure should be mapped");
    assert!(matches!(
        stderr_tee_error,
        CommandError::WriteOutputFailed {
            stream: OutputStream::Stderr,
            output: Some(_),
            ..
        }
    ));

    let stdin_error = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Ok(CapturedOutput::default())),
        output_reader(Ok(CapturedOutput::default())),
        Some(thread::spawn(|| -> io::Result<()> {
            panic!("coverage stdin writer failure");
        })),
    )
    .expect_err("coverage stdin failure should be returned");
    assert!(matches!(stdin_error, CommandError::WriteInputFailed { .. }));

    let output = collect_output(
        "command",
        status(),
        || Ok(Duration::from_secs(1)),
        output_reader(Ok(CapturedOutput {
            bytes: b"out".to_vec(),
            truncated: true,
        })),
        output_reader(Ok(CapturedOutput {
            bytes: b"err".to_vec(),
            truncated: false,
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
    })))
    .expect("successful coverage reader should join");
    assert_eq!(joined.bytes, b"ok");
    let joined_error = join_output_reader(output_reader(Err(
        OutputCaptureError::Read(io::Error::other("coverage read failure")),
    )))
    .expect_err("coverage reader error should be preserved");
    assert!(matches!(joined_error, OutputCaptureError::Read(_)));
    let joined_panic = join_output_reader(thread::spawn(
        || -> Result<_, OutputCaptureError> {
            panic!("coverage reader panic");
        },
    ))
    .expect_err("coverage reader panic should map to a read error");
    assert!(matches!(joined_panic, OutputCaptureError::Read(_)));

    let mut no_pipe = ProcessCommand::new("rustc")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("coverage child should spawn");
    let missing_stdin =
        write_stdin_bytes("command", &mut no_pipe, Some(b"input".to_vec()))
            .expect_err("coverage missing stdin pipe should be reported");
    assert!(matches!(
        missing_stdin,
        CommandError::WriteInputFailed { .. }
    ));
    no_pipe.wait().expect("coverage child should be waitable");

    let mut piped = ProcessCommand::new("rustc")
        .arg("--version")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("coverage child should spawn with stdin");
    let writer =
        write_stdin_bytes("command", &mut piped, Some(b"input".to_vec()))
            .expect("coverage stdin writer should start")
            .expect("coverage stdin writer should exist");
    join_stdin_writer("command", Some(writer))
        .expect("coverage stdin writer should finish");
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
    let injected_error =
        write_stdin_bytes("command", &mut injected, Some(b"input".to_vec()))
            .expect_err("coverage stdin thread failure should be injected");
    __coverage_fail_stdin_thread(false);
    assert!(matches!(
        injected_error,
        CommandError::StartInputThreadFailed { .. }
    ));
    injected.kill().expect("injected child should be stoppable");
    injected.wait().expect("injected child should be waitable");
    let mapped_stdin_error = map_stdin_thread_result(
        "command",
        Err(io::Error::other("coverage thread start failure")),
    )
    .expect_err("coverage thread start mapping should preserve the error");
    assert!(matches!(
        mapped_stdin_error,
        CommandError::StartInputThreadFailed { .. }
    ));

    join_stdin_writer(
        "command",
        Some(thread::spawn(|| {
            Err::<(), io::Error>(io::Error::from(io::ErrorKind::BrokenPipe))
        })),
    )
    .expect("coverage broken pipe should be accepted");
    let stdin_write_error = join_stdin_writer(
        "command",
        Some(thread::spawn(|| {
            Err::<(), io::Error>(io::Error::other("coverage stdin failure"))
        })),
    )
    .expect_err("coverage stdin failure should be mapped");
    assert!(matches!(
        stdin_write_error,
        CommandError::WriteInputFailed { .. }
    ));
    let stdin_panic = join_stdin_writer(
        "command",
        Some(thread::spawn(|| -> io::Result<()> {
            panic!("coverage stdin panic");
        })),
    )
    .expect_err("coverage stdin panic should be mapped");
    assert!(matches!(stdin_panic, CommandError::WriteInputFailed { .. }));
    join_stdin_writer("command", None)
        .expect("coverage absent writer should be accepted");

    let child = spawn_rustc_child();
    drop(StartingCommand::new("coverage-cleanup", child));

    let mut exited_child = spawn_rustc_child();
    process_wrap::std::ChildWrapper::wait(exited_child.as_mut())
        .expect("coverage child should exit before cleanup");
    drop(StartingCommand::new(
        "coverage-exited-cleanup",
        exited_child,
    ));
}
