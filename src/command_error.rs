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

use thiserror::Error;

use crate::{
    CommandOutput,
    OutputStream,
};

/// Error returned while spawning, waiting for, or validating a command.
///
/// # Author
///
/// Haixing Hu
#[derive(Debug, Error)]
pub enum CommandError {
    /// The process could not be spawned.
    #[error("failed to spawn command `{command}`: {source}")]
    SpawnFailed {
        /// Human-readable command representation.
        command: String,
        /// I/O error reported by the operating system.
        source: io::Error,
    },

    /// Waiting for process completion failed.
    #[error("failed to wait for command `{command}`: {source}")]
    WaitFailed {
        /// Human-readable command representation.
        command: String,
        /// I/O error reported while waiting for the child process.
        source: io::Error,
    },

    /// The process could not be killed after exceeding the configured timeout.
    #[error("failed to kill timed-out command `{command}` after {timeout:?}: {source}")]
    KillFailed {
        /// Human-readable command representation.
        command: String,
        /// Timeout that was exceeded.
        timeout: Duration,
        /// I/O error reported while killing the child process.
        source: io::Error,
    },

    /// Reading one of the captured output streams failed.
    #[error("failed to read {stream} for command `{command}`: {source}")]
    ReadOutputFailed {
        /// Human-readable command representation.
        command: String,
        /// Stream whose reader failed.
        stream: OutputStream,
        /// I/O error reported while reading the stream.
        source: io::Error,
    },

    /// The command exceeded the configured timeout and was terminated.
    #[error("command `{command}` timed out after {timeout:?}")]
    TimedOut {
        /// Human-readable command representation.
        command: String,
        /// Timeout that was exceeded.
        timeout: Duration,
        /// Captured output available after terminating the child process.
        output: Box<CommandOutput>,
    },

    /// The command completed with an exit code not configured as successful.
    #[error("command `{command}` exited with code {exit_code:?}; expected one of {expected:?}")]
    UnexpectedExit {
        /// Human-readable command representation.
        command: String,
        /// Exit code reported by the process, if available.
        exit_code: Option<i32>,
        /// Configured successful exit codes.
        expected: Vec<i32>,
        /// Captured output from the failed command.
        output: Box<CommandOutput>,
    },
}

impl CommandError {
    /// Returns captured command output when this error carries it.
    ///
    /// # Returns
    ///
    /// `Some(output)` for timeout and unexpected-exit errors, otherwise `None`.
    #[inline]
    pub const fn output(&self) -> Option<&CommandOutput> {
        match self {
            Self::TimedOut { output, .. } | Self::UnexpectedExit { output, .. } => Some(output),
            _ => None,
        }
    }

    /// Returns the command string associated with this error.
    ///
    /// # Returns
    ///
    /// A human-readable command representation used in diagnostics.
    #[inline]
    pub fn command(&self) -> &str {
        match self {
            Self::SpawnFailed { command, .. }
            | Self::WaitFailed { command, .. }
            | Self::KillFailed { command, .. }
            | Self::ReadOutputFailed { command, .. }
            | Self::TimedOut { command, .. }
            | Self::UnexpectedExit { command, .. } => command,
        }
    }
}
