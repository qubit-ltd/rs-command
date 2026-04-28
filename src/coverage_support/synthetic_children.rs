/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    ffi::OsStr,
    process::{
        ChildStderr,
        ChildStdout,
        Command as SyntheticCommand,
        Stdio,
    },
};

use crate::{
    OutputStream,
    command_runner::managed_child_process::ManagedChildProcess,
};

use super::NoStdinChild;

/// Creates a synthetic child for coverage-only run-loop branches.
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
pub(crate) fn forced_collect_output_error(command: &str) -> Option<OutputStream> {
    if command.contains("__qubit_command_collect_output_error__")
        || command.contains("__qubit_command_timeout_collect_output_error__")
    {
        Some(OutputStream::Stdout)
    } else {
        None
    }
}

fn child_with_output_pipes() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        ..NoStdinChild::default()
    }
}

fn child_pending_with_output_pipes() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        stderr: Some(empty_stderr()),
        pending: true,
        ..NoStdinChild::default()
    }
}

fn child_with_stdout_only() -> NoStdinChild {
    NoStdinChild {
        stdout: Some(empty_stdout()),
        ..NoStdinChild::default()
    }
}

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
