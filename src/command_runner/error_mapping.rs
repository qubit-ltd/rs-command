/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    io,
    time::Duration,
};

use crate::{
    CommandError,
    OutputStream,
};

/// Builds a process spawn failure.
pub(crate) fn spawn_failed(command: &str, source: io::Error) -> CommandError {
    CommandError::SpawnFailed {
        command: command.to_owned(),
        source,
    }
}

/// Builds a process wait failure.
pub(crate) fn wait_failed(command: &str, source: io::Error) -> CommandError {
    CommandError::WaitFailed {
        command: command.to_owned(),
        source,
    }
}

/// Builds a timed-out process kill failure.
pub(crate) fn kill_failed(command: String, timeout: Duration, source: io::Error) -> CommandError {
    CommandError::KillFailed {
        command,
        timeout,
        source,
    }
}

/// Builds an internal missing-pipe error.
pub(crate) fn output_pipe_error(command: &str, stream: OutputStream) -> CommandError {
    CommandError::ReadOutputFailed {
        command: command.to_owned(),
        stream,
        source: io::Error::other(format!("{} pipe was not created", stream.as_str())),
    }
}
