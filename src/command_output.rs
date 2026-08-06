// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::{
    borrow::Cow,
    fmt,
    process::ExitStatus,
    str,
    time::Duration,
};

use qubit_redact::redacted_debug;

/// Captured output and status information from a finished command.
///
/// `CommandOutput` stores retained raw stdout and stderr bytes. When the runner
/// is configured with per-stream capture limits, the retained bytes may be a
/// prefix of the full output; use [`Self::stdout_truncated`] and
/// [`Self::stderr_truncated`] to detect that case. If a timeout or
/// cancellation interrupts collection, use [`Self::stdout_complete`] and
/// [`Self::stderr_complete`] to detect partial streams. [`Self::stdout`] and
/// [`Self::stderr`] return raw bytes exactly as retained. Use
/// [`Self::stdout_text`] and [`Self::stderr_text`] for strict UTF-8 text, or
/// [`Self::stdout_lossy_text`] and [`Self::stderr_lossy_text`] to replace
/// invalid byte sequences with the Unicode replacement character. Its
/// [`fmt::Debug`] implementation redacts both captured streams and reports
/// only their retained lengths, truncation flags, and completion flags.
///
/// # Examples
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_command::{Command, CommandOutput, CommandRunner};
///
/// fn run_command() -> CommandOutput {
///     CommandRunner::new(std::time::Duration::from_secs(10))
///         .run(Command::new("true"))
///         .unwrap()
/// }
///
/// run_command();
/// ```
#[derive(Clone, PartialEq, Eq)]
#[must_use]
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
    /// Whether stdout reached EOF before collection was cancelled.
    stdout_complete: bool,
    /// Whether stderr reached EOF before collection was cancelled.
    stderr_complete: bool,
    /// Duration from process spawn until output collection completes.
    elapsed: Duration,
}

impl fmt::Debug for CommandOutput {
    /// Formats process metadata while redacting both captured streams.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Destination formatter.
    ///
    /// # Returns
    ///
    /// Formatting result after rendering redacted output metadata.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandOutput")
            .field("status", &self.status)
            .field("stdout", &redacted_debug(&self.stdout))
            .field("stdout_len", &self.stdout.len())
            .field("stdout_truncated", &self.stdout_truncated)
            .field("stdout_complete", &self.stdout_complete)
            .field("stderr", &redacted_debug(&self.stderr))
            .field("stderr_len", &self.stderr.len())
            .field("stderr_truncated", &self.stderr_truncated)
            .field("stderr_complete", &self.stderr_complete)
            .field("elapsed", &self.elapsed)
            .finish()
    }
}

impl CommandOutput {
    /// Creates command output from captured process data.
    ///
    /// # Parameters
    ///
    /// * `status` - Process exit status.
    /// * `stdout` - Captured stdout bytes, truncation flag, and completion
    ///   flag.
    /// * `stderr` - Captured stderr bytes, truncation flag, and completion
    ///   flag.
    /// * `elapsed` - Observed process duration.
    /// # Returns
    ///
    /// A command output value containing the supplied data.
    #[inline]
    pub(crate) fn new(
        status: ExitStatus,
        stdout: (Vec<u8>, bool, bool),
        stderr: (Vec<u8>, bool, bool),
        elapsed: Duration,
    ) -> Self {
        let (stdout, stdout_truncated, stdout_complete) = stdout;
        let (stderr, stderr_truncated, stderr_complete) = stderr;
        Self {
            status,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            stdout_complete,
            stderr_complete,
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
    #[inline(always)]
    pub fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }

    /// Returns the full process exit status.
    ///
    /// # Returns
    ///
    /// Platform-specific process exit status reported by the operating system.
    #[must_use]
    #[inline(always)]
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
    #[inline(always)]
    pub fn termination_signal(&self) -> Option<i32> {
        self.status.signal()
    }

    /// Returns captured standard output bytes.
    ///
    /// # Returns
    ///
    /// A borrowed slice containing stdout exactly as retained by the runner.
    ///
    /// Check [`Self::stdout_truncated`] to determine whether retained bytes are
    /// only a prefix of the process output.
    #[must_use]
    #[inline(always)]
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    /// Consumes this output and returns captured standard output bytes.
    ///
    /// # Returns
    ///
    /// Owned stdout bytes exactly as retained by the runner. This avoids a
    /// copy when the caller no longer needs the remaining output metadata.
    #[must_use]
    #[inline(always)]
    pub fn into_stdout(self) -> Vec<u8> {
        self.stdout
    }

    /// Returns captured standard error bytes.
    ///
    /// # Returns
    ///
    /// A borrowed slice containing stderr exactly as retained by the runner.
    ///
    /// Check [`Self::stderr_truncated`] to determine whether retained bytes are
    /// only a prefix of the process output.
    #[must_use]
    #[inline(always)]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    /// Returns whether stdout was drained through EOF.
    ///
    /// A `false` value means collection was cancelled, so the retained bytes
    /// may be only a prefix of the process output even when no capture limit
    /// was configured.
    #[must_use]
    #[inline(always)]
    pub const fn stdout_complete(&self) -> bool {
        self.stdout_complete
    }

    /// Returns whether stderr was drained through EOF.
    ///
    /// A `false` value means collection was cancelled, so the retained bytes
    /// may be only a prefix of the process output even when no capture limit
    /// was configured.
    #[must_use]
    #[inline(always)]
    pub const fn stderr_complete(&self) -> bool {
        self.stderr_complete
    }

    /// Consumes this output and returns captured standard error bytes.
    ///
    /// # Returns
    ///
    /// Owned stderr bytes exactly as retained by the runner. This avoids a
    /// copy when the caller no longer needs the remaining output metadata.
    #[must_use]
    #[inline(always)]
    pub fn into_stderr(self) -> Vec<u8> {
        self.stderr
    }

    /// Returns captured standard output as strict UTF-8 text.
    ///
    /// # Returns
    ///
    /// `Ok(&str)` when stdout is valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`str::Utf8Error`] when retained stdout contains invalid UTF-8.
    /// A capture limit can retain only part of a multi-byte sequence, so this
    /// error does not necessarily mean the process emitted invalid UTF-8.
    #[inline(always)]
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
    /// Returns [`str::Utf8Error`] when retained stderr contains invalid UTF-8.
    /// A capture limit can retain only part of a multi-byte sequence, so this
    /// error does not necessarily mean the process emitted invalid UTF-8.
    #[inline(always)]
    pub fn stderr_text(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(&self.stderr)
    }

    /// Returns captured standard output as UTF-8 text, replacing invalid bytes.
    ///
    /// # Returns
    ///
    /// Borrowed UTF-8 text when stdout is valid UTF-8, or an owned string with
    /// invalid byte sequences replaced by the Unicode replacement character.
    #[must_use]
    #[inline(always)]
    pub fn stdout_lossy_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    /// Returns captured standard error as UTF-8 text, replacing invalid bytes.
    ///
    /// # Returns
    ///
    /// Borrowed UTF-8 text when stderr is valid UTF-8, or an owned string with
    /// invalid byte sequences replaced by the Unicode replacement character.
    #[must_use]
    #[inline(always)]
    pub fn stderr_lossy_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }

    /// Returns the observed command duration.
    ///
    /// # Returns
    ///
    /// Duration from process spawn until final output collection. This may
    /// include time spent draining inherited output pipes after the direct
    /// child process exits.
    #[must_use]
    #[inline(always)]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Returns whether captured stdout was truncated by a configured limit.
    ///
    /// # Returns
    ///
    /// `true` when stdout emitted more bytes than the runner retained.
    #[must_use]
    #[inline(always)]
    pub const fn stdout_truncated(&self) -> bool {
        self.stdout_truncated
    }

    /// Returns whether captured stderr was truncated by a configured limit.
    ///
    /// # Returns
    ///
    /// `true` when stderr emitted more bytes than the runner retained.
    #[must_use]
    #[inline(always)]
    pub const fn stderr_truncated(&self) -> bool {
        self.stderr_truncated
    }
}
