// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fmt;

/// Standard output or error stream identified in command diagnostics.
///
/// ```
/// use qubit_command::{Command, CommandRunner, OutputStream};
///
/// let output = CommandRunner::without_timeout()
///     .run(Command::new("rustc").arg("--version"))
///     .expect("rustc should run");
/// assert_eq!(OutputStream::Stdout.as_str(), "stdout");
/// assert!(!output.stdout().is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum OutputStream {
    /// Standard output stream.
    Stdout,

    /// Standard error stream.
    Stderr,
}

impl OutputStream {
    /// Returns a lowercase stream name for diagnostics.
    ///
    /// # Returns
    ///
    /// `"stdout"` for [`Self::Stdout`] and `"stderr"` for [`Self::Stderr`].
    #[must_use]
    #[inline(always)]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

impl fmt::Display for OutputStream {
    /// Formats this stream name for diagnostics.
    ///
    /// # Parameters
    ///
    /// * `f` - Formatter receiving the lowercase stream name.
    ///
    /// # Returns
    ///
    /// [`fmt::Result`] from writing the stream name.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
