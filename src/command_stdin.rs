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
///
/// This type stays internal so the public builder API can evolve without
/// exposing process-spawning details.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum CommandStdin {
    /// Connect stdin to null input.
    Null,
    /// Inherit stdin from the parent process.
    Inherit,
    /// Write these bytes to the child process stdin.
    Bytes(Vec<u8>),
    /// Read stdin bytes from this file.
    File(PathBuf),
}

impl fmt::Debug for CommandStdin {
    /// Formats stdin configuration without exposing inline bytes.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Destination formatter.
    ///
    /// # Returns
    ///
    /// Formatting result after rendering stdin kind and safe metadata.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => formatter.write_str("Null"),
            Self::Inherit => formatter.write_str("Inherit"),
            Self::Bytes(bytes) => {
                write!(formatter, "Bytes({} bytes)", bytes.len())
            }
            Self::File(_) => formatter
                .debug_tuple("File")
                .field(&"<redacted path>")
                .finish(),
        }
    }
}
