/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::path::PathBuf;

/// Standard input configuration for a command.
///
/// This type stays internal so the public builder API can evolve without
/// exposing process-spawning details.
#[derive(Debug, Clone, PartialEq, Eq)]
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
