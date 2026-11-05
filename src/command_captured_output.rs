// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Captured stdout and stderr bytes with collection metadata.

use std::fmt;

/// Output bytes retained by a command runner, including partial output on
/// failure. Raw bytes can contain sensitive data; the `Debug` output reports
/// lengths and metadata without rendering those bytes.
///
/// Each stream is kept independently. Truncation means the retained bytes are
/// a prefix limited by the runner's configured per-stream cap; completion
/// means that stream reached EOF, even when tee output failed. A command error
/// may carry this value without an [`ExitStatus`](std::process::ExitStatus):
/// for example, an output-budget error has no final process status, while
/// [`CommandError::captured_output`](crate::CommandError::captured_output)
/// still exposes the bytes observed before termination.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use qubit_command::{Command, CommandErrorKind, CommandErrorReason, CommandRunner};
///
/// let output = CommandRunner::new(Duration::from_secs(10))
///     .max_stdout_bytes(0)
///     .fail_on_output_truncation(false)
///     .run(Command::new("rustc").arg("--version"))
///     .expect("rustc should run");
/// assert!(output.stdout().is_empty());
/// assert!(output.stdout_truncated());
/// assert!(output.stdout_complete());
///
/// let error = CommandRunner::new(Duration::from_secs(10))
///     .max_output_bytes(8)
///     .max_total_output_bytes(0)
///     .run(Command::new("rustc").arg("--version"))
///     .expect_err("the first output byte should exceed the zero-byte budget");
/// assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
/// assert!(matches!(error.reason(),
///     CommandErrorReason::OutputLimitExceeded { limit: 0, observed: 1 }));
/// assert!(error.output().is_none());
/// let captured = error.captured_output().expect("budget error should retain bytes");
/// assert_eq!(captured.stdout().len(), 1);
/// assert!(!captured.stdout_truncated());
/// assert!(!captured.stdout_complete());
/// ```
#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct CommandCapturedOutput {
    /// Retained stdout bytes, which may be only a prefix of the stream.
    pub(crate) stdout: Vec<u8>,
    /// Retained stderr bytes, which may be only a prefix of the stream.
    pub(crate) stderr: Vec<u8>,
    /// Whether stdout exceeded its retention cap.
    stdout_truncated: bool,
    /// Whether stderr exceeded its retention cap.
    stderr_truncated: bool,
    /// Whether stdout EOF was observed.
    stdout_complete: bool,
    /// Whether stderr EOF was observed.
    stderr_complete: bool,
}

impl CommandCapturedOutput {
    /// Creates captured output from retained bytes and stream metadata.
    ///
    /// This constructor is crate-private; callers obtain values from command
    /// results and errors.
    ///
    /// # Parameters
    ///
    /// * `stdout` - Retained bytes, truncation flag, and EOF flag for stdout.
    /// * `stderr` - Retained bytes, truncation flag, and EOF flag for stderr.
    ///
    /// # Returns
    ///
    /// Captured bytes with their per-stream collection metadata.
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

    /// Returns the retained stdout bytes.
    ///
    /// The bytes may be a prefix of the full stream when
    /// [`Self::stdout_truncated`] is true.
    ///
    /// # Returns
    ///
    /// A borrowed slice of retained raw bytes.
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }
    /// Returns the retained stderr bytes.
    ///
    /// The bytes may be a prefix of the full stream when
    /// [`Self::stderr_truncated`] is true.
    ///
    /// # Returns
    ///
    /// A borrowed slice of retained raw bytes.
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }
    /// Whether stdout exceeded its in-memory retention limit.
    ///
    /// This is independent of whether the stream reached EOF.
    ///
    /// # Returns
    ///
    /// `true` when bytes were omitted due to the retention cap.
    pub const fn stdout_truncated(&self) -> bool {
        self.stdout_truncated
    }
    /// Whether stderr exceeded its in-memory retention limit.
    ///
    /// This is independent of whether the stream reached EOF.
    ///
    /// # Returns
    ///
    /// `true` when bytes were omitted due to the retention cap.
    pub const fn stderr_truncated(&self) -> bool {
        self.stderr_truncated
    }
    /// Whether stdout reached EOF.
    ///
    /// `false` does not identify why collection stopped; inspect the error
    /// reason. Completion is independent of truncation.
    ///
    /// # Returns
    ///
    /// `true` when EOF was observed.
    pub const fn stdout_complete(&self) -> bool {
        self.stdout_complete
    }
    /// Whether stderr reached EOF.
    ///
    /// `false` does not identify why collection stopped; inspect the error
    /// reason. Completion is independent of truncation.
    ///
    /// # Returns
    ///
    /// `true` when EOF was observed.
    pub const fn stderr_complete(&self) -> bool {
        self.stderr_complete
    }

    /// Consumes the captured output and returns owned stdout and stderr bytes.
    ///
    /// The bytes are moved without copying. This discards the per-stream
    /// truncation and completion metadata, so inspect those flags before
    /// calling this method when they are relevant. Truncated bytes may be only
    /// a prefix of the process output, and incomplete streams did not reach
    /// observed EOF. Treat the returned raw bytes as sensitive according to
    /// your application's data-handling requirements.
    ///
    /// # Returns
    ///
    /// A tuple containing `(stdout, stderr)` exactly as retained by the
    /// runner.
    ///
    /// # Examples
    ///
    /// ```
    /// use qubit_command::{Command, CommandRunner};
    ///
    /// let captured = CommandRunner::without_timeout()
    ///     .run(Command::new("rustc").arg("--version"))
    ///     .expect("rustc should run")
    ///     .into_captured_output();
    /// let (stdout, stderr) = captured.into_streams();
    /// assert!(!stdout.is_empty());
    /// assert!(stderr.is_empty());
    /// ```
    #[inline]
    pub fn into_streams(self) -> (Vec<u8>, Vec<u8>) {
        (self.stdout, self.stderr)
    }
}

impl fmt::Debug for CommandCapturedOutput {
    /// Formats lengths and stream metadata without exposing raw bytes.
    ///
    /// # Parameters
    ///
    /// * `f` - Formatter receiving the redacted summary.
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
