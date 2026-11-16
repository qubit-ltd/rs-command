// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::CommandSubmitFailureReason;
use crate::Command;
use crate::CommandResourceRunOptions;
use crate::CommandRunner;

/// Inputs returned when resource submission is rejected.
#[derive(Debug)]
pub struct CommandResourceSubmitFailure {
    /// Stores the runner value used by this execution component.
    pub runner: Box<CommandRunner>,
    /// Stores the command value used by this execution component.
    pub command: Box<Command>,
    /// Stores the options value used by this execution component.
    pub options: Box<CommandResourceRunOptions>,
    /// Stores the reason value used by this execution component.
    pub reason: CommandSubmitFailureReason,
}
