/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::{
    borrow::Cow,
    process::ExitStatus,
    str,
    time::Duration,
};

/// Captured output and status information from a finished command.
///
/// `CommandOutput` stores retained raw stdout and stderr bytes. When the runner
/// is configured with per-stream capture limits, the retained bytes may be a
/// prefix of the full output; use [`Self::stdout_truncated`] and
/// [`Self::stderr_truncated`] to detect that case. [`Self::stdout`] and
/// [`Self::stderr`] return raw bytes exactly as retained. Use
/// [`Self::stdout_text`] and [`Self::stderr_text`] for strict UTF-8 text, or
/// [`Self::stdout_lossy_text`] and [`Self::stderr_lossy_text`] to replace
/// invalid byte sequences with the Unicode replacement character.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit status reported by the process.
    status: ExitStatus,
    /// Captured standard output bytes.
    stdout: Vec<u8>,
    /// Captured standard error bytes.
    stderr: Vec<u8>,
    /// Whether stdout was truncated by the configured capture limit.
    stdout_truncated: bool,
    /// Whether stderr was truncated by the configured capture limit.
    stderr_truncated: bool,
    /// Duration from process spawn to observed termination.
    elapsed: Duration,
}

impl CommandOutput {
    /// Creates command output from captured process data.
    ///
    /// # Parameters
    ///
    /// * `status` - Process exit status.
    /// * `stdout` - Captured standard output bytes.
    /// * `stderr` - Captured standard error bytes.
    /// * `stdout_truncated` - Whether stdout exceeded the capture limit.
    /// * `stderr_truncated` - Whether stderr exceeded the capture limit.
    /// * `elapsed` - Observed process duration.
    /// # Returns
    ///
    /// A command output value containing the supplied data.
    #[inline]
    pub(crate) fn new(
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        stdout_truncated: bool,
        stderr_truncated: bool,
        elapsed: Duration,
    ) -> Self {
        Self {
            status,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            elapsed,
        }
    }

    /// Returns the command exit code.
    ///
    /// # Returns
    ///
    /// `Some(code)` when the platform reports a numeric process exit code, or
    /// `None` when the process ended in a way that does not map to a numeric
    /// code.
    #[inline]
    pub fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }

    /// Returns the full process exit status.
    ///
    /// # Returns
    ///
    /// Platform-specific process exit status reported by the operating system.
    #[inline]
    pub const fn exit_status(&self) -> &ExitStatus {
        &self.status
    }

    /// Returns the signal that terminated the process on Unix platforms.
    ///
    /// # Returns
    ///
    /// `Some(signal)` when the process was terminated by a signal, otherwise
    /// `None`.
    #[cfg(unix)]
    #[inline]
    pub fn termination_signal(&self) -> Option<i32> {
        self.status.signal()
    }

    /// Returns captured standard output bytes.
    ///
    /// # Returns
    ///
    /// A borrowed slice containing stdout exactly as emitted by the process.
    #[inline]
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    /// Returns captured standard error bytes.
    ///
    /// # Returns
    ///
    /// A borrowed slice containing stderr exactly as emitted by the process.
    #[inline]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    /// Returns captured standard output as strict UTF-8 text.
    ///
    /// # Returns
    ///
    /// `Ok(&str)` when stdout is valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`str::Utf8Error`] when stdout contains invalid UTF-8.
    #[inline]
    pub fn stdout_text(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(&self.stdout)
    }

    /// Returns captured standard error as strict UTF-8 text.
    ///
    /// # Returns
    ///
    /// `Ok(&str)` when stderr is valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`str::Utf8Error`] when stderr contains invalid UTF-8.
    #[inline]
    pub fn stderr_text(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(&self.stderr)
    }

    /// Returns captured standard output as UTF-8 text, replacing invalid bytes.
    ///
    /// # Returns
    ///
    /// Borrowed UTF-8 text when stdout is valid UTF-8, or an owned string with
    /// invalid byte sequences replaced by the Unicode replacement character.
    #[inline]
    pub fn stdout_lossy_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    /// Returns captured standard error as UTF-8 text, replacing invalid bytes.
    ///
    /// # Returns
    ///
    /// Borrowed UTF-8 text when stderr is valid UTF-8, or an owned string with
    /// invalid byte sequences replaced by the Unicode replacement character.
    #[inline]
    pub fn stderr_lossy_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }

    /// Returns the observed command duration.
    ///
    /// # Returns
    ///
    /// Duration from process spawn to observed termination.
    #[inline]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Returns whether captured stdout was truncated by a configured limit.
    ///
    /// # Returns
    ///
    /// `true` when stdout emitted more bytes than the runner retained.
    #[inline]
    pub const fn stdout_truncated(&self) -> bool {
        self.stdout_truncated
    }

    /// Returns whether captured stderr was truncated by a configured limit.
    ///
    /// # Returns
    ///
    /// `true` when stderr emitted more bytes than the runner retained.
    #[inline]
    pub const fn stderr_truncated(&self) -> bool {
        self.stderr_truncated
    }
}
