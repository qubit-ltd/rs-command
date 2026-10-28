// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Captured stdout and stderr bytes with collection metadata.

use std::fmt;

/// Output bytes retained by a command runner, including partial output on
/// failure.
#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct CommandCapturedOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    stdout_complete: bool,
    stderr_complete: bool,
}

impl CommandCapturedOutput {
    pub(crate) fn new(stdout: (Vec<u8>, bool, bool), stderr: (Vec<u8>, bool, bool)) -> Self {
        let (stdout, stdout_truncated, stdout_complete) = stdout;
        let (stderr, stderr_truncated, stderr_complete) = stderr;
        Self {
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            stdout_complete,
            stderr_complete,
        }
    }

    /// Returns retained stdout bytes.
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }
    /// Returns retained stderr bytes.
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }
    /// Whether stdout exceeded its in-memory retention limit.
    pub const fn stdout_truncated(&self) -> bool {
        self.stdout_truncated
    }
    /// Whether stderr exceeded its in-memory retention limit.
    pub const fn stderr_truncated(&self) -> bool {
        self.stderr_truncated
    }
    /// Whether stdout reached EOF.
    pub const fn stdout_complete(&self) -> bool {
        self.stdout_complete
    }
    /// Whether stderr reached EOF.
    pub const fn stderr_complete(&self) -> bool {
        self.stderr_complete
    }
}

impl fmt::Debug for CommandCapturedOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandCapturedOutput")
            .field("stdout_len", &self.stdout.len())
            .field("stdout_truncated", &self.stdout_truncated)
            .field("stdout_complete", &self.stdout_complete)
            .field("stderr_len", &self.stderr.len())
            .field("stderr_truncated", &self.stderr_truncated)
            .field("stderr_complete", &self.stderr_complete)
            .finish()
    }
}
