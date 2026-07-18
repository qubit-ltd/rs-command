// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::process::Command as ProcessCommand;

use crate::Command;

/// Configures environment variables for a process command.
///
/// # Parameters
///
/// * `command` - Structured command containing environment changes.
/// * `process_command` - Standard-library command to update.
///
/// This mutates only the process builder; the operating-system environment is
/// not changed until that command is spawned.
pub(super) fn configure_environment(
    command: &Command,
    process_command: &mut ProcessCommand,
) {
    if command.clears_environment() {
        process_command.env_clear();
    }
    for key in command.removed_environment() {
        process_command.env_remove(key);
    }
    for (key, value) in command.environment() {
        process_command.env(key, value);
    }
}
