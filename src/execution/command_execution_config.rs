// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::CommandExecutorCreateError;

/// Limits retained by a command executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandExecutionConfig {
    /// Stores the max active value used by this execution component.
    pub max_active: usize,
    /// Stores the max queued value used by this execution component.
    pub max_queued: usize,
    /// Stores the max waiters value used by this execution component.
    pub max_waiters: usize,
    /// Stores the max queued input bytes value used by this execution
    /// component.
    pub max_queued_input_bytes: usize,
    /// Stores the max diagnostics value used by this execution component.
    pub max_diagnostics: usize,
}
impl Default for CommandExecutionConfig {
    /// Performs the default step in the managed command execution lifecycle.
    fn default() -> Self {
        Self {
            max_active: 4,
            max_queued: 64,
            max_waiters: 64,
            max_queued_input_bytes: 16 * 1024 * 1024,
            max_diagnostics: 64,
        }
    }
}
impl CommandExecutionConfig {
    /// Performs the validate step in the managed command execution lifecycle.
    pub fn validate(&self) -> Result<(), CommandExecutorCreateError> {
        if self.max_active == 0 {
            return Err(CommandExecutorCreateError::ZeroActiveCapacity);
        }
        Ok(())
    }
}
