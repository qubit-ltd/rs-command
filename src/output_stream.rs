/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::fmt;

/// Output stream whose reader failed.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    #[inline]
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
