// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::CommandExecutorCreateError;

/// Executor-local admission limits for managed command submissions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandExecutionConfig {
    /// Maximum number of active command reservations.
    ///
    /// The default is 4. This value must be nonzero.
    pub max_active: usize,
    /// Maximum number of queued command reservations, excluding active ones.
    ///
    /// The default is 64. A value of 0 disables queueing but still permits
    /// immediate admission when an active slot is available.
    pub max_queued: usize,
    /// Maximum number of callers registered while waiting for admission
    /// capacity.
    ///
    /// The default is 64. A value of 0 disables capacity waiters; it does not
    /// prevent immediate admission. Waiting callers may still hold their
    /// submission inputs while they wait.
    pub max_waiters: usize,
    /// Maximum estimated bytes retained by active and queued reservations.
    ///
    /// The default is 16 MiB. Despite the legacy name, this is not limited to
    /// queued work and is not a memory limit. Standard command options
    /// estimate to zero. Resource options estimate the sum of the metadata
    /// lengths of their present stdin, stdout, and stderr files; existing
    /// output contents count, and stdout/stderr are counted before any
    /// truncation. The estimate uses each file's full length, not its
    /// remaining readable bytes. Metadata lookup failures contribute zero.
    /// The estimate is accumulated with saturation. A value of 0 admits only
    /// submissions estimated at zero bytes.
    pub max_queued_input_bytes: usize,
    /// Compatibility field with no effect on diagnostic retention.
    ///
    /// This field is not read by the executor. All values, including zero,
    /// have the same behavior. New callers should not rely on this field.
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
