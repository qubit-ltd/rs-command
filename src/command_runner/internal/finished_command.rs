// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::CommandOutput;

/// Retains a command's diagnostic text and collected output after its process
/// and I/O helpers have finished.
pub(in crate::command_runner) struct FinishedCommand {
    /// Redacted command text used to identify the completed command in
    /// diagnostics and logs.
    pub(in crate::command_runner) command_text: String,
    /// Captured status, output bytes, truncation and completion state, and
    /// elapsed time; streams may be partial when collection was interrupted.
    pub(in crate::command_runner) output: CommandOutput,
}
