// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::CommandSubmitFailureReason;
use crate::Command;
use crate::CommandRunOptions;
use crate::CommandRunner;

/// Inputs returned when ordinary submission is rejected before ownership
/// transfers.
#[derive(Debug)]
pub struct CommandSubmitFailure {
    /// Stores the runner value used by this execution component.
    pub runner: Box<CommandRunner>,
    /// Stores the command value used by this execution component.
    pub command: Box<Command>,
    /// Stores the options value used by this execution component.
    pub options: CommandRunOptions,
    /// Stores the reason value used by this execution component.
    pub reason: CommandSubmitFailureReason,
}
