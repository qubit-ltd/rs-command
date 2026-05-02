/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use crate::CommandOutput;

/// Output of a command whose process and I/O helpers have completed.
pub(crate) struct FinishedCommand {
    /// Human-readable command text for diagnostics and logging.
    pub(crate) command_text: String,
    /// Captured command output.
    pub(crate) output: CommandOutput,
}
