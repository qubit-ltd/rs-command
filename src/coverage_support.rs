/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Coverage-only hooks for exercising defensive process-runner branches.

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::{
    cell::{
        Cell,
        RefCell,
    },
    ffi::OsStr,
    io::{
        self,
        Write,
    },
    panic,
    path::PathBuf,
    process::{
        ChildStderr,
        ChildStdout,
        Command as SyntheticCommand,
        ExitStatus,
        Stdio,
    },
    thread,
    time::Duration,
};

use process_wrap::std::ChildWrapper;

mod failing_flush;
mod failing_reader;
mod failing_write;
mod fake_child_guard;
mod no_stdin_child;

use failing_flush::FailingFlush;
use failing_reader::FailingReader;
use failing_write::FailingWrite;
use fake_child_guard::FakeChildGuard;
use no_stdin_child::NoStdinChild;

use crate::{
    OutputStream,
    command_runner::{
        WAIT_POLL_INTERVAL,
        captured_output::CapturedOutput,
        collect_output,
        join_output_reader,
        join_stdin_writer,
        kill_failed,
        managed_child_process::ManagedChildProcess,
        next_sleep,
        output_capture_error::OutputCaptureError,
        output_capture_options::OutputCaptureOptions,
        output_pipe_error,
        output_reader::OutputReader,
        read_output,
        spawn_failed,
        wait_failed,
        write_stdin_bytes,
    },
};

thread_local! {
    /// Whether synthetic children are enabled on this test thread.
    static FAKE_CHILDREN_ENABLED: Cell<bool> = const { Cell::new(false) };
    /// Commands whose output collection path has been reached.
    static COLLECT_OUTPUT_COMMANDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Runs an operation with coverage-only synthetic children enabled.
///
/// # Parameters
///
/// * `operation` - Operation that may run magic coverage-only command names.
///
/// # Returns
///
/// The value returned by `operation`.
pub fn with_fake_children_enabled<T>(operation: impl FnOnce() -> T) -> T {
    let _guard = enable_fake_children();
    operation()
}

/// Returns whether coverage-only synthetic children are enabled.
///
/// # Returns
///
/// `true` only within [`with_fake_children_enabled`] on the current thread.
pub(crate) fn fake_children_enabled() -> bool {
    FAKE_CHILDREN_ENABLED.get()
}

/// Records that output collection was reached for a command.
///
/// # Parameters
///
/// * `command` - Human-readable command text passed to output collection.
pub(crate) fn record_collect_output(command: &str) {
    COLLECT_OUTPUT_COMMANDS.with_borrow_mut(|commands| commands.push(command.to_owned()));
}

/// Takes and clears recorded output-collection commands.
///
/// # Returns
///
/// Recorded command texts since the previous call on this thread.
pub fn take_collect_output_commands() -> Vec<String> {
    COLLECT_OUTPUT_COMMANDS.take()
}

/// Enables coverage-only synthetic children for the current thread.
///
/// # Returns
///
/// Guard restoring the previous state when dropped.
fn enable_fake_children() -> FakeChildGuard {
    let previous = fake_children_enabled();
    FAKE_CHILDREN_ENABLED.set(true);
    FakeChildGuard { previous }
}

/// Exercises internal error helpers that cannot be reached reliably through
/// real OS process execution.
///
/// # Returns
///
/// Diagnostic strings built from each exercised error path.
pub fn exercise_defensive_paths() -> Vec<String> {
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

/// Creates a successful exit status for coverage-only helper calls.
///
/// # Returns
///
/// Platform-specific successful process exit status.
fn success_status() -> ExitStatus {
    ExitStatus::from_raw(0)
}

/// Creates an output reader that succeeds with the supplied bytes.
///
/// # Parameters
///
/// * `bytes` - Bytes returned by the synthetic reader.
///
/// # Returns
///
/// Output reader join handle.
fn reader_ok(bytes: Vec<u8>) -> OutputReader {
    thread::spawn(move || {
        Ok(CapturedOutput {
            bytes,
            truncated: false,
        })
    })
}

/// Creates an output reader that fails with a read error.
///
/// # Parameters
///
/// * `message` - Error message used by the synthetic reader.
///
/// # Returns
///
/// Output reader join handle.
fn reader_read_error(message: &'static str) -> OutputReader {
    thread::spawn(move || Err(OutputCaptureError::Read(io::Error::other(message))))
}

/// Creates an output reader that fails with a tee-write error.
///
/// # Parameters
///
/// * `message` - Error message used by the synthetic writer.
///
/// # Returns
///
/// Output reader join handle.
fn reader_write_error(message: &'static str) -> OutputReader {
    thread::spawn(move || {
        Err(OutputCaptureError::Write {
            path: PathBuf::from("reader-output.txt"),
            source: io::Error::other(message),
        })
    })
}

/// Creates a synthetic child for coverage-only run-loop branches.
///
/// # Parameters
///
/// * `program` - Program name passed to the process command.
///
/// # Returns
///
/// A synthetic child for known coverage-only program names, otherwise `None` so
/// normal process spawning proceeds.
pub(crate) fn fake_child_for(program: &OsStr) -> Option<ManagedChildProcess> {
    let child = match program.to_string_lossy().as_ref() {
        "__qubit_command_missing_stdin__" => child_with_output_pipes(),
        "__qubit_command_missing_stdout__" => NoStdinChild::default(),
        "__qubit_command_missing_stderr__" => child_with_stdout_only(),
        "__qubit_command_try_wait_error__" => child_with_try_wait_error(),
        "__qubit_command_try_wait_error_kill_cleanup__" => child_with_try_wait_error_kill_cleanup(),
        "__qubit_command_try_wait_error_pending_after_kill__" => {
            child_with_try_wait_error_pending_after_kill()
        }
        "__qubit_command_kill_error__" => child_with_kill_error(),
        "__qubit_command_wait_after_kill_error__" => child_with_wait_after_kill_error(),
        "__qubit_command_collect_output_error__" => child_with_output_pipes(),
        "__qubit_command_timeout_collect_output_error__" => child_pending_with_output_pipes(),
        _ => return None,
    };
    Some(Box::new(child))
}

/// Checks whether output collection should fail for a synthetic command.
///
/// # Parameters
///
/// * `command` - Human-readable command text built by the runner.
///
/// # Returns
///
/// The stream to report as failed for known synthetic command names, otherwise
/// `None`.
pub(crate) fn forced_collect_output_error(command: &str) -> Option<OutputStream> {
    if command.contains("__qubit_command_collect_output_error__")
        || command.contains("__qubit_command_timeout_collect_output_error__")
    {
        Some(OutputStream::Stdout)
    } else {
        None
    }
}

/// Creates a synthetic child with both output pipes available.
///
/// # Returns
///
/// Child wrapper state used to reach output collection through the normal
/// process completion branch.
fn child_with_output_pipes() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        ..NoStdinChild::default()
    }
}

/// Creates a pending synthetic child with both output pipes available.
///
/// # Returns
///
/// Child wrapper state used to reach output collection through the timeout
/// branch.
fn child_pending_with_output_pipes() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        ..NoStdinChild::default()
    }
}

/// Creates a synthetic child with only stdout available.
///
/// # Returns
///
/// Child wrapper state used to exercise missing-stderr handling.
fn child_with_stdout_only() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        ..NoStdinChild::default()
    }
}

/// Creates a synthetic child whose try-wait operation fails.
///
/// # Returns
///
/// Child wrapper state used to exercise wait-error cleanup when the child exits
/// after a kill request.
fn child_with_try_wait_error() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        try_wait_error: Some("try wait failed"),
        clear_try_wait_error_after_first: true,
        pending: true,
        exited_after_kill_attempt: true,
        ..NoStdinChild::default()
    }
}

/// Creates a synthetic child whose wait-error cleanup uses the fallback.
///
/// # Returns
///
/// Child wrapper state used to exercise cleanup after try-wait and kill errors.
fn child_with_try_wait_error_kill_cleanup() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        try_wait_error: Some("try wait failed"),
        clear_try_wait_error_after_first: true,
        pending: true,
        kill_error: Some("cleanup kill failed"),
        exited_after_kill_attempt: true,
        ..NoStdinChild::default()
    }
}

/// Creates a synthetic child that remains pending after wait-error cleanup.
///
/// # Returns
///
/// Child wrapper state used to verify wait-error cleanup does not use a blocking
/// wait when the child is not confirmed to have exited.
fn child_with_try_wait_error_pending_after_kill() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        try_wait_error: Some("try wait failed"),
        clear_try_wait_error_after_first: true,
        pending: true,
        ..NoStdinChild::default()
    }
}

/// Creates a synthetic child whose kill operation fails.
///
/// # Returns
///
/// Child wrapper state used to exercise kill-error handling.
fn child_with_kill_error() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        kill_error: Some("kill failed"),
        exited_after_kill_attempt: true,
        ..NoStdinChild::default()
    }
}

/// Creates a synthetic child whose wait after kill fails.
///
/// # Returns
///
/// Child wrapper state used to exercise post-timeout wait-error handling.
fn child_with_wait_after_kill_error() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        wait_error: Some("wait after kill failed"),
        exited_after_kill_attempt: true,
        ..NoStdinChild::default()
    }
}

/// Creates an empty stdout pipe handle.
///
/// # Returns
///
/// A child stdout handle that is already at EOF.
fn empty_stdout() -> ChildStdout {
    let mut child = empty_process_command()
        .stdout(Stdio::piped())
        .spawn()
        .expect("synthetic stdout child should spawn");
    let stdout = child
        .stdout
        .take()
        .expect("synthetic stdout should be piped");
    child.wait().expect("synthetic stdout child should finish");
    stdout
}

/// Creates an empty stderr pipe handle.
///
/// # Returns
///
/// A child stderr handle that is already at EOF.
fn empty_stderr() -> ChildStderr {
    let mut child = empty_process_command()
        .stderr(Stdio::piped())
        .spawn()
        .expect("synthetic stderr child should spawn");
    let stderr = child
        .stderr
        .take()
        .expect("synthetic stderr should be piped");
    child.wait().expect("synthetic stderr child should finish");
    stderr
}

/// Creates a platform shell command that exits without output.
///
/// # Returns
///
/// A process command used only to obtain pipe handles for synthetic children.
fn empty_process_command() -> SyntheticCommand {
    #[cfg(not(windows))]
    {
        let mut command = SyntheticCommand::new("sh");
        command.arg("-c").arg(":");
        command
    }
    #[cfg(windows)]
    {
        let mut command = SyntheticCommand::new("cmd");
        command.arg("/C").arg("exit /B 0");
        command
    }
}
