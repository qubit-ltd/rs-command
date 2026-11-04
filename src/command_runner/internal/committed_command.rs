// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::process::Command as ProcessCommand;

use super::file_tee::FileTee;

/// Command whose I/O resources have passed the commit point.
pub(in crate::command_runner) struct CommittedCommand {
    /// Human-readable command text for logs and diagnostics.
    pub(in crate::command_runner) command_text: String,
    /// Process command ready to spawn.
    pub(in crate::command_runner) process_command: ProcessCommand,
    /// Bytes to write to stdin after spawning, if configured.
    pub(in crate::command_runner) stdin_bytes: Option<Vec<u8>>,
    /// Paired stdout tee resource, if configured.
    pub(in crate::command_runner) stdout_tee: Option<FileTee>,
    /// Paired stderr tee resource, if configured.
    pub(in crate::command_runner) stderr_tee: Option<FileTee>,
}
