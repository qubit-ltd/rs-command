/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
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

/// Spawns a child process with platform process-tree support.
pub(crate) fn spawn_child(
    process_command: ProcessCommand,
    kill_process_tree: bool,
) -> io::Result<ManagedChildProcess> {
    let mut command = CommandWrap::from(process_command);
    #[cfg(unix)]
    if kill_process_tree {
        command.wrap(ProcessGroup::leader());
    }
    #[cfg(windows)]
    if kill_process_tree {
        command.wrap(JobObject);
    }
    command.spawn()
}
