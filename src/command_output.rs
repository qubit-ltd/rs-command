// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::borrow::Cow;
use std::fmt;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::str;
use std::time::Duration;

use qubit_redact::Redactor;

use crate::CommandCapturedOutput;

/// Redacts one debug-only value before it crosses the diagnostic boundary.
///
/// # Parameters
///
/// * `value` - Value whose debug representation may contain sensitive data.
///
/// # Returns
///
/// A redacted representation, or a marker when redaction cannot be completed.
fn redacted_debug_text(value: &impl fmt::Debug) -> String {
    Redactor::strict()
        .redact_field("command_output", &format_args!("{value:?}"))
        .into_text_or_marker("<redaction incomplete>")
        .into_string()
}

/// Captured output and status information from a finished command.
///
/// `CommandOutput` stores retained raw stdout and stderr bytes. When the runner
/// is configured with per-stream capture limits, the retained bytes may be a
/// prefix of the full output; use [`Self::stdout_truncated`] and
/// [`Self::stderr_truncated`] to detect that case. If collection stops before
/// EOF because of a read failure, cancellation, or
/// output-budget termination, use [`Self::stdout_complete`] and
/// [`Self::stderr_complete`] to detect that condition. These flags report
/// whether EOF was observed, not why collection stopped. Completion is
/// independent of truncation: a stream can reach EOF after its retained bytes
/// were truncated. A tee flush failure can also occur after a stream reached
/// EOF. [`Self::stdout`] and
/// [`Self::stderr`] return raw bytes exactly as retained. Use
/// [`Self::stdout_text`] and [`Self::stderr_text`] for strict UTF-8 text, or
/// [`Self::stdout_lossy_text`] and [`Self::stderr_lossy_text`] to replace
/// invalid byte sequences with the Unicode replacement character. Its
/// [`fmt::Debug`] implementation redacts both captured streams and reports
/// only their retained lengths, truncation flags, and completion flags.
///
/// # Examples
///
/// ```
/// use qubit_command::{Command, CommandOutput, CommandRunner};
///
/// let output: CommandOutput = CommandRunner::without_timeout()
///     .run(Command::new("rustc").arg("--version"))
///     .expect("rustc should run");
/// assert!(!output.stdout().is_empty());
/// ```
#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct CommandOutput {
    /// Exit status reported by the process.
    status: ExitStatus,
    /// Captured standard output bytes.
    captured: CommandCapturedOutput,
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
            .field("stdout", &redacted_debug_text(&self.captured.stdout))
            .field("stderr", &redacted_debug_text(&self.captured.stderr))
            .field("captured", &self.captured)
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
        Self {
            status,
            captured: CommandCapturedOutput::new(stdout, stderr),
            elapsed,
        }
    }

    /// Consumes the command output and returns its captured streams and
    /// metadata.
    ///
    /// The returned value retains the truncation and completion flags for each
    /// stream. Truncation means the bytes may be only a prefix of the process
    /// output; a `false` completion flag means EOF was not observed and does
    /// not identify why collection stopped. This moves the existing buffers
    /// without copying them.
    ///
    /// # Examples
    ///
    /// ```
    /// use qubit_command::{Command, CommandRunner};
    ///
    /// let output = CommandRunner::without_timeout()
    ///     .run(Command::new("rustc").arg("--version"))
    ///     .expect("rustc should run");
    /// let captured = output.into_captured_output();
    /// let (stdout, stderr) = captured.into_streams();
    /// assert!(!stdout.is_empty());
    /// assert!(stderr.is_empty());
    /// ```
    #[inline]
    pub fn into_captured_output(self) -> CommandCapturedOutput {
        self.captured
    }

    /// Returns the command exit code.
    ///
    /// # Returns
    ///
    /// `Some(code)` when the platform reports a numeric process exit code, or
    /// `None` when the process ended in a way that does not map to a numeric
    /// code.
    #[must_use]
    #[inline]
    pub fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }

    /// Returns the full process exit status.
    ///
    /// # Returns
    ///
    /// Platform-specific process exit status reported by the operating system.
    #[must_use]
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
    #[must_use]
    #[inline]
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
    #[inline]
    pub fn stdout(&self) -> &[u8] {
        self.captured.stdout()
    }

    /// Consumes this output and returns captured standard output bytes.
    ///
    /// # Returns
    ///
    /// Owned stdout bytes exactly as retained by the runner. This avoids a
    /// copy when the caller no longer needs the remaining output metadata.
    #[must_use]
    #[inline]
    pub fn into_stdout(self) -> Vec<u8> {
        self.captured.stdout
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
    #[inline]
    pub fn stderr(&self) -> &[u8] {
        self.captured.stderr()
    }

    /// Returns whether stdout reached EOF.
    ///
    /// A `false` value means EOF was not observed. Collection may have stopped
    /// because of a read failure, cancellation, or output-budget termination;
    /// inspect the command error for the reason. This is independent of
    /// truncation, and a tee flush failure may occur after EOF.
    ///
    /// # Returns
    ///
    /// `true` when stdout reached EOF; `false` when EOF was not observed.
    #[must_use]
    #[inline]
    pub const fn stdout_complete(&self) -> bool {
        self.captured.stdout_complete()
    }

    /// Returns whether stderr reached EOF.
    ///
    /// A `false` value means EOF was not observed. Collection may have stopped
    /// because of a read failure, cancellation, or output-budget termination;
    /// inspect the command error for the reason. This is independent of
    /// truncation, and a tee flush failure may occur after EOF.
    ///
    /// # Returns
    ///
    /// `true` when stderr reached EOF; `false` when EOF was not observed.
    #[must_use]
    #[inline]
    pub const fn stderr_complete(&self) -> bool {
        self.captured.stderr_complete()
    }

    /// Consumes this output and returns captured standard error bytes.
    ///
    /// # Returns
    ///
    /// Owned stderr bytes exactly as retained by the runner. This avoids a
    /// copy when the caller no longer needs the remaining output metadata.
    #[must_use]
    #[inline]
    pub fn into_stderr(self) -> Vec<u8> {
        self.captured.stderr
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
    #[inline]
    pub fn stdout_text(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(self.captured.stdout())
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
    #[inline]
    pub fn stderr_text(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(self.captured.stderr())
    }

    /// Returns captured standard output as UTF-8 text, replacing invalid bytes.
    ///
    /// # Returns
    ///
    /// Borrowed UTF-8 text when stdout is valid UTF-8, or an owned string with
    /// invalid byte sequences replaced by the Unicode replacement character.
    #[must_use]
    #[inline]
    pub fn stdout_lossy_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.captured.stdout())
    }

    /// Returns captured standard error as UTF-8 text, replacing invalid bytes.
    ///
    /// # Returns
    ///
    /// Borrowed UTF-8 text when stderr is valid UTF-8, or an owned string with
    /// invalid byte sequences replaced by the Unicode replacement character.
    #[must_use]
    #[inline]
    pub fn stderr_lossy_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.captured.stderr())
    }

    /// Returns the observed command duration.
    ///
    /// # Returns
    ///
    /// Duration from process spawn until final output collection. This may
    /// include time spent draining inherited output pipes after the direct
    /// child process exits.
    #[must_use]
    #[inline]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Returns whether captured stdout was truncated by a configured limit.
    ///
    /// # Returns
    ///
    /// `true` when stdout emitted more bytes than the runner retained.
    #[must_use]
    #[inline]
    pub const fn stdout_truncated(&self) -> bool {
        self.captured.stdout_truncated()
    }

    /// Returns whether captured stderr was truncated by a configured limit.
    ///
    /// # Returns
    ///
    /// `true` when stderr emitted more bytes than the runner retained.
    #[must_use]
    #[inline]
    pub const fn stderr_truncated(&self) -> bool {
        self.captured.stderr_truncated()
    }

    /// Returns captured stdout and stderr bytes with truncation and completion
    /// metadata.
    #[inline]
    pub const fn captured_output(&self) -> &CommandCapturedOutput {
        &self.captured
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::CommandOutput;

    #[test]
    fn test_into_captured_output_moves_nonempty_stream_buffers() {
        let stdout = vec![1, 2, 3, 4];
        let stderr = vec![5, 6, 7];
        let stdout_ptr = stdout.as_ptr();
        let stderr_ptr = stderr.as_ptr();
        let output = CommandOutput::new(
            test_status(),
            (stdout, false, true),
            (stderr, false, true),
            Duration::ZERO,
        );

        let captured = output.into_captured_output();

        assert_eq!(captured.stdout().as_ptr(), stdout_ptr);
        assert_eq!(captured.stderr().as_ptr(), stderr_ptr);
        assert_eq!(captured.stdout(), [1, 2, 3, 4]);
        assert_eq!(captured.stderr(), [5, 6, 7]);
        assert!(captured.stdout_complete());
        assert!(captured.stderr_complete());
    }

    #[test]
    fn test_into_captured_output_preserves_incomplete_and_truncated_flags() {
        let output = CommandOutput::new(
            test_status(),
            (vec![1], true, false),
            (vec![2], false, false),
            Duration::ZERO,
        );

        assert!(output.stdout_truncated());
        assert!(!output.stdout_complete());
        assert!(!output.stderr_truncated());
        assert!(!output.stderr_complete());

        let captured = output.into_captured_output();
        assert!(captured.stdout_truncated());
        assert!(!captured.stdout_complete());
        assert!(!captured.stderr_truncated());
        assert!(!captured.stderr_complete());
        assert_eq!(captured.stdout(), [1]);
        assert_eq!(captured.stderr(), [2]);
    }

    #[cfg(unix)]
    fn test_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }

    #[cfg(windows)]
    fn test_status() -> std::process::ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }
}
