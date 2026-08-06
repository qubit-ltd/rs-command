// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    io,
    process::Command as ProcessCommand,
};

use process_wrap::std::CommandWrap;
#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;

use super::managed_child_process::ManagedChildProcess;

/// Spawns a child process with optional platform process-tree support.
///
/// # Parameters
///
/// * `process_command` - Fully prepared standard-library process command.
/// * `manage_process_tree` - Whether timeout or cancellation handling must own
///   the child process tree.
///
/// # Returns
///
/// Managed child wrapper owning the spawned process.
///
/// # Errors
///
/// Returns the operating-system spawn error, including failures to configure
/// the process group or Job Object.
pub(in crate::command_runner) fn spawn_child(
    process_command: ProcessCommand,
    manage_process_tree: bool,
) -> io::Result<ManagedChildProcess> {
    let mut command = CommandWrap::from(process_command);
    #[cfg(unix)]
    if manage_process_tree {
        command.wrap(ProcessGroup::leader());
    }
    #[cfg(windows)]
    if manage_process_tree {
        command.wrap(JobObject);
    }
    let child_process = command.spawn()?;
    Ok(ManagedChildProcess::new(
        child_process,
        manage_process_tree,
    ))
}
