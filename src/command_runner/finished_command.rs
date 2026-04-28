/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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
