// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal representation of per-run command options.

use std::path::PathBuf;

use crate::CommandCancellation;

/// Owned run options transferred from the public builder to the runner.
#[derive(Clone)]
pub(crate) struct CommandRunOptionsParts {
    /// Optional cancellation handle transferred to the running command.
    pub(crate) cancellation: Option<CommandCancellation>,
    /// Optional stdout tee path transferred to the output helper.
    pub(crate) stdout_file: Option<PathBuf>,
    /// Optional stderr tee path transferred to the output helper.
    pub(crate) stderr_file: Option<PathBuf>,
}
