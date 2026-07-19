// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    io,
    time::Duration,
};

use crate::{
    CommandError,
    OutputStream,
};

/// Builds a process spawn failure.
///
/// # Parameters
///
/// * `command` - Sanitized command text.
/// * `source` - Process-spawn error.
///
/// # Returns
///
/// Structured spawn failure retaining the command and source.
#[must_use]
#[inline]
pub(in crate::command_runner) fn spawn_failed(
    command: &str,
    source: io::Error,
) -> CommandError {
    CommandError::SpawnFailed {
        command: command.to_owned(),
        source,
    }
}

/// Builds a process wait failure.
///
/// # Parameters
///
/// * `command` - Sanitized command text.
/// * `source` - Process-wait error.
///
/// # Returns
///
/// Structured wait failure retaining the command and source.
#[must_use]
#[inline]
pub(in crate::command_runner) fn wait_failed(
    command: &str,
    source: io::Error,
) -> CommandError {
    CommandError::WaitFailed {
        command: command.to_owned(),
        source,
    }
}

/// Builds a timed-out process kill failure.
///
/// # Parameters
///
/// * `command` - Sanitized command text.
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
    source: io::Error,
) -> CommandError {
    CommandError::KillFailed {
        command,
        timeout,
        source,
    }
}

/// Builds an internal missing-pipe error.
///
/// # Parameters
///
/// * `command` - Sanitized command text.
/// * `stream` - Missing child output stream.
///
/// # Returns
///
/// Structured output-read failure describing the missing pipe.
#[must_use]
#[inline]
pub(in crate::command_runner) fn output_pipe_error(
    command: &str,
    stream: OutputStream,
) -> CommandError {
    CommandError::ReadOutputFailed {
        command: command.to_owned(),
        stream,
        source: io::Error::other(format!(
            "{} pipe was not created",
            stream.as_str()
        )),
    }
}
