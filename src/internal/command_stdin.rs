// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fmt;
use std::path::PathBuf;

/// Standard input configuration for a command.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum CommandStdin {
    /// Connect stdin to null input.
    Null,
    /// Inherit stdin from the parent process.
    Inherit,
    /// Write these bytes to the child process stdin.
    Bytes(
        /// Bytes written to the child before stdin is closed.
        Vec<u8>,
    ),
    /// Read stdin bytes from this file.
    File(
        /// Path to the regular file opened before spawning the child.
        PathBuf,
    ),
}

/// Command stdin category needed for managed resource compatibility checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandStdinMode {
    /// Null or inherited stdin, which supplies no explicit file or byte input.
    Default,
    /// Buffered bytes supplied as command stdin.
    Bytes,
    /// File path supplied as command stdin.
    File,
}

impl fmt::Debug for CommandStdin {
    /// Formats the input mode while hiding file paths and byte contents.
    ///
    /// This representation is intended for diagnostics and does not expose
    /// sensitive stdin data.
    ///
    /// # Parameters
    ///
    /// * `self` — the configured stdin source.
    /// * `formatter` — the destination for the redacted representation.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => formatter.write_str("Null"),
            Self::Inherit => formatter.write_str("Inherit"),
            Self::Bytes(bytes) => {
                write!(formatter, "Bytes({} bytes)", bytes.len())
            }
            Self::File(_) => formatter.debug_tuple("File").field(&"<redacted path>").finish(),
        }
    }
}
