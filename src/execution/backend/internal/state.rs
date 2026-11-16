// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::StopReason;

use crate::CommandCancellation;

/// Represents the state state or contract used by managed command execution.
#[derive(Debug, Default)]
pub(in crate::execution::backend) struct State {
    /// Stores the stopped value used by this execution component.
    pub(in crate::execution::backend) stopped: bool,
    /// Stores the authorized value used by this execution component.
    pub(in crate::execution::backend) authorized: bool,
    /// Stores the external cancellation value used by this execution component.
    pub(in crate::execution::backend) external_cancellation: Option<CommandCancellation>,
    /// Stores the reason value used by this execution component.
    pub(in crate::execution::backend) reason: Option<StopReason>,
}
