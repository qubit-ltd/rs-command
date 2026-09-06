// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;
use std::time::Duration;

use crate::CommandError;
use crate::CommandErrorReason;
use crate::OutputStream;

/// Builds a process spawn failure.
///
/// # Parameters
///
/// * `command` - Redacted command text.
/// * `source` - Process-spawn error.
///
/// # Returns
///
/// Structured spawn failure retaining the command and source.
#[must_use]
#[inline]
pub(in crate::command_runner) fn spawn_failed(command: &str, source: io::Error) -> CommandError {
    CommandError::from_reason(command, CommandErrorReason::SpawnFailed { source }, None)
}

/// Builds a process wait failure.
///
/// # Parameters
///
/// * `command` - Redacted command text.
/// * `source` - Process-wait error.
///
/// # Returns
///
/// Structured wait failure retaining the command and source.
#[must_use]
#[inline]
pub(in crate::command_runner) fn wait_failed(command: &str, source: io::Error) -> CommandError {
    CommandError::from_reason(command, CommandErrorReason::WaitFailed { source }, None)
}

/// Builds a timed-out process kill failure.
///
/// # Parameters
///
/// * `command` - Redacted command text.
/// * `timeout` - Timeout exceeded by the command.
/// * `source` - Process-termination error.
///
/// # Returns
///
/// Structured kill failure retaining the command, timeout, and source.
#[must_use]
#[inline]
pub(in crate::command_runner) fn kill_failed(
    command: String,
    timeout: Duration,
    process_tree_source: io::Error,
    child_source: io::Error,
) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::KillFailed {
            timeout,
            process_tree_source,
            child_source,
        },
        None,
    )
}

/// Builds an internal missing-pipe error.
///
/// # Parameters
///
/// * `command` - Redacted command text.
/// * `stream` - Missing child output stream.
///
/// # Returns
///
/// Structured output-read failure describing the missing pipe.
#[must_use]
#[inline]
pub(in crate::command_runner) fn output_pipe_error(command: &str, stream: OutputStream) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::ReadOutputFailed {
            stream,
            source: io::Error::other(format!("{} pipe was not created", stream.as_str())),
        },
        None,
    )
}
