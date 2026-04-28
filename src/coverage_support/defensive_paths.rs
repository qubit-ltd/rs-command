/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::{
    ffi::OsStr,
    io::{self, Write},
    panic,
    path::PathBuf,
    process::ExitStatus,
    thread,
    time::Duration,
};

use process_wrap::std::ChildWrapper;

use crate::{
    OutputStream,
    command_runner::{
        captured_output::CapturedOutput,
        error_mapping::{kill_failed, output_pipe_error, spawn_failed, wait_failed},
        output_capture_error::OutputCaptureError,
        output_capture_options::OutputCaptureOptions,
        output_collector::{collect_output, join_output_reader, read_output},
        output_reader::OutputReader,
        stdin_pipe::{join_stdin_writer, write_stdin_bytes},
        wait_policy::{WAIT_POLL_INTERVAL, next_sleep},
    },
};

use super::{
    FailingFlush, FailingReader, FailingWrite, NoStdinChild, fake_child_for, fake_children_enabled,
    with_fake_children_enabled,
};

/// Exercises internal error helpers that cannot be reached reliably through
/// real OS process execution.
pub(super) fn exercise_defensive_paths() -> Vec<String> {
    let mut diagnostics = vec![
        spawn_failed("spawn", io::Error::other("spawn failed")).to_string(),
        wait_failed("wait", io::Error::other("wait failed")).to_string(),
        kill_failed(
            "kill".to_owned(),
            Duration::from_millis(1),
            io::Error::other("kill failed"),
        )
        .to_string(),
        output_pipe_error("pipe", OutputStream::Stdout).to_string(),
        output_pipe_error("pipe", OutputStream::Stderr).to_string(),
    ];

    let mut failing_reader = FailingReader;
    let read_error = read_output(
        &mut failing_reader,
        OutputCaptureOptions::new(None, None, None),
    )
    .expect_err("failing reader should report read error");
    if let OutputCaptureError::Read(source) = read_error {
        diagnostics.push(source.to_string());
    }

    let mut write_reader = io::Cursor::new(b"write".to_vec());
    let write_error = read_output(
        &mut write_reader,
        OutputCaptureOptions::new_writer(None, Box::new(FailingWrite), PathBuf::from("stdout.txt")),
    )
    .expect_err("failing writer should report write error");
    if let OutputCaptureError::Write { path, source } = write_error {
        diagnostics.push(path.display().to_string());
        diagnostics.push(source.to_string());
    }

    let mut flush_reader = io::Cursor::new(b"flush".to_vec());
    let flush_error = read_output(
        &mut flush_reader,
        OutputCaptureOptions::new_writer(None, Box::new(FailingFlush), PathBuf::from("stderr.txt")),
    )
    .expect_err("failing flush should report write error");
    if let OutputCaptureError::Write { path, source } = flush_error {
        diagnostics.push(path.display().to_string());
        diagnostics.push(source.to_string());
    }

    let mut no_stdin_child = NoStdinChild::default();
    diagnostics.push(format!("{}", no_stdin_child.id()));
    diagnostics.push(format!("{}", no_stdin_child.inner().id()));
    diagnostics.push(format!("{}", no_stdin_child.inner_mut().id()));
    diagnostics.push(format!("{}", no_stdin_child.stdout().is_none()));
    diagnostics.push(format!("{}", no_stdin_child.stderr().is_none()));
    diagnostics.push(format!(
        "{}",
        no_stdin_child
            .try_wait()
            .expect("synthetic try_wait should succeed")
            .is_some(),
    ));
    no_stdin_child
        .start_kill()
        .expect("synthetic kill should succeed");
    diagnostics.push(format!(
        "{}",
        no_stdin_child
            .wait()
            .expect("synthetic wait should succeed")
            .success(),
    ));
    diagnostics.push(
        write_stdin_bytes(
            "missing-stdin",
            &mut no_stdin_child,
            Some(b"input".to_vec()),
        )
        .expect_err("missing child stdin should be reported")
        .to_string(),
    );
    let boxed_child = Box::new(NoStdinChild::default()).into_inner();
    diagnostics.push(format!("{}", boxed_child.id()));
    let mut persistent_try_wait_error_child = NoStdinChild {
        try_wait_error: Some("persistent try wait failed"),
        ..NoStdinChild::default()
    };
    diagnostics.push(
        persistent_try_wait_error_child
            .try_wait()
            .expect_err("synthetic persistent try-wait error should be reported")
            .to_string(),
    );
    diagnostics.push(
        persistent_try_wait_error_child
            .try_wait()
            .expect_err("synthetic persistent try-wait error should remain configured")
            .to_string(),
    );

    let mut failing_write = FailingWrite;
    failing_write
        .flush()
        .expect("synthetic flush should succeed");

    let failed_stdout = reader_read_error("collect stdout failed");
    let empty_stderr = reader_ok(Vec::new());
    diagnostics.push(
        collect_output(
            "collect-stdout",
            success_status(),
            Duration::ZERO,
            false,
            failed_stdout,
            empty_stderr,
            None,
        )
        .expect_err("stdout collection error should be mapped")
        .to_string(),
    );

    let empty_stdout = reader_ok(Vec::new());
    let failed_stderr = reader_read_error("collect stderr failed");
    diagnostics.push(
        collect_output(
            "collect-stderr",
            success_status(),
            Duration::ZERO,
            false,
            empty_stdout,
            failed_stderr,
            None,
        )
        .expect_err("stderr collection error should be mapped")
        .to_string(),
    );

    let empty_stdout = reader_ok(Vec::new());
    let empty_stderr = reader_ok(Vec::new());
    diagnostics.push(
        collect_output(
            "collect-stdin",
            success_status(),
            Duration::ZERO,
            false,
            empty_stdout,
            empty_stderr,
            Some(thread::spawn(|| {
                Err(io::Error::other("collect stdin failed"))
            })),
        )
        .expect_err("stdin collection error should be mapped")
        .to_string(),
    );

    let reader_error = reader_read_error("reader failed");
    diagnostics.push(
        join_output_reader("reader", OutputStream::Stdout, reader_error)
            .expect_err("reader error should be mapped")
            .to_string(),
    );

    let writer_error = reader_write_error("reader write failed");
    diagnostics.push(
        join_output_reader("writer", OutputStream::Stdout, writer_error)
            .expect_err("writer error should be mapped")
            .to_string(),
    );

    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let panicked_reader = thread::spawn(|| -> Result<CapturedOutput, OutputCaptureError> {
        panic!("output reader panic");
    });
    let panic_error = join_output_reader("panic", OutputStream::Stderr, panicked_reader)
        .expect_err("reader panic should be mapped")
        .to_string();
    panic::set_hook(previous_hook);
    diagnostics.push(panic_error);

    diagnostics.push(
        join_stdin_writer(
            "stdin-write",
            Some(thread::spawn(|| {
                Err(io::Error::other("stdin write failed"))
            })),
        )
        .expect_err("stdin writer error should be mapped")
        .to_string(),
    );

    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let stdin_panicked = thread::spawn(|| -> io::Result<()> {
        panic!("stdin writer panic");
    });
    let stdin_panic_error = join_stdin_writer("stdin-panic", Some(stdin_panicked))
        .expect_err("stdin writer panic should be mapped")
        .to_string();
    panic::set_hook(previous_hook);
    diagnostics.push(stdin_panic_error);

    diagnostics.push(format!("{:?}", next_sleep(None, Duration::ZERO)));
    diagnostics.push(format!(
        "{:?}",
        next_sleep(Some(Duration::from_millis(1)), Duration::from_millis(2)),
    ));
    diagnostics.push(format!(
        "{:?}",
        next_sleep(Some(Duration::from_secs(1)), Duration::ZERO),
    ));
    diagnostics.push(format!("{WAIT_POLL_INTERVAL:?}"));
    diagnostics.push(format!(
        "{}",
        fake_child_for(OsStr::new("__qubit_command_normal_child__")).is_none(),
    ));
    diagnostics.push(format!("{}", fake_children_enabled()));
    with_fake_children_enabled(|| {
        diagnostics.push(format!("{}", fake_children_enabled()));
    });
    diagnostics.push(format!("{}", fake_children_enabled()));
    diagnostics
}

fn success_status() -> ExitStatus {
    ExitStatus::from_raw(0)
}

fn reader_ok(bytes: Vec<u8>) -> OutputReader {
    thread::spawn(move || {
        Ok(CapturedOutput {
            bytes,
            truncated: false,
        })
    })
}

fn reader_read_error(message: &'static str) -> OutputReader {
    thread::spawn(move || Err(OutputCaptureError::Read(io::Error::other(message))))
}

fn reader_write_error(message: &'static str) -> OutputReader {
    thread::spawn(move || {
        Err(OutputCaptureError::Write {
            path: PathBuf::from("reader-output.txt"),
            source: io::Error::other(message),
        })
    })
}
