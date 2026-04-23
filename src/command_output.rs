/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    borrow::Cow,
    str,
    time::Duration,
};

/// Captured output and status information from a finished command.
///
/// `CommandOutput` stores raw bytes because external programs do not guarantee
/// UTF-8 output. Use [`Self::stdout_utf8`], [`Self::stderr_utf8`],
/// [`Self::stdout_lossy`], or [`Self::stderr_lossy`] when text is needed.
///
/// # Author
///
/// Haixing Hu
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit code reported by the process, or `None` when the platform could not
    /// represent termination as a numeric code.
    exit_code: Option<i32>,
    /// Captured standard output bytes.
    stdout: Vec<u8>,
    /// Captured standard error bytes.
    stderr: Vec<u8>,
    /// Duration from process spawn to observed termination.
    elapsed: Duration,
}

impl CommandOutput {
    /// Creates command output from captured process data.
    ///
    /// # Parameters
    ///
    /// * `exit_code` - Numeric process exit code, if available.
    /// * `stdout` - Captured standard output bytes.
    /// * `stderr` - Captured standard error bytes.
    /// * `elapsed` - Observed process duration.
    ///
    /// # Returns
    ///
    /// A command output value containing the supplied data.
    #[inline]
    pub(crate) const fn new(
        exit_code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        elapsed: Duration,
    ) -> Self {
        Self {
            exit_code,
            stdout,
            stderr,
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
    pub const fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Returns the captured standard output bytes.
    ///
    /// # Returns
    ///
    /// A borrowed slice containing stdout exactly as emitted by the process.
    #[inline]
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    /// Returns the captured standard error bytes.
    ///
    /// # Returns
    ///
    /// A borrowed slice containing stderr exactly as emitted by the process.
    #[inline]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
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

    /// Interprets captured standard output as UTF-8.
    ///
    /// # Returns
    ///
    /// `Ok(&str)` if stdout is valid UTF-8, otherwise returns the UTF-8
    /// validation error.
    ///
    /// # Errors
    ///
    /// Returns [`str::Utf8Error`] when stdout contains invalid UTF-8 bytes.
    #[inline]
    pub fn stdout_utf8(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(&self.stdout)
    }

    /// Interprets captured standard error as UTF-8.
    ///
    /// # Returns
    ///
    /// `Ok(&str)` if stderr is valid UTF-8, otherwise returns the UTF-8
    /// validation error.
    ///
    /// # Errors
    ///
    /// Returns [`str::Utf8Error`] when stderr contains invalid UTF-8 bytes.
    #[inline]
    pub fn stderr_utf8(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(&self.stderr)
    }

    /// Converts standard output to text, replacing invalid UTF-8 sequences.
    ///
    /// # Returns
    ///
    /// A borrowed or owned string containing a lossy UTF-8 representation of
    /// stdout.
    #[inline]
    pub fn stdout_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    /// Converts standard error to text, replacing invalid UTF-8 sequences.
    ///
    /// # Returns
    ///
    /// A borrowed or owned string containing a lossy UTF-8 representation of
    /// stderr.
    #[inline]
    pub fn stderr_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }
}
