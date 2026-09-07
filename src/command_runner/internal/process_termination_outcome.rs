// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Successful process-termination outcome.

use std::process::ExitStatus;

use crate::CommandCleanupFailure;

/// Successful process termination together with non-fatal cleanup failures.
#[derive(Debug)]
pub(super) struct ProcessTerminationOutcome {
    /// Final status reported by the direct child.
    pub(super) status: ExitStatus,
    /// Non-fatal termination failures observed before the final status.
    pub(super) cleanup_failures: Vec<CommandCleanupFailure>,
}
