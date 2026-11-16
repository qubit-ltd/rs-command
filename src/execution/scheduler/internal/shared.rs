// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Condvar;
use std::sync::Mutex;

use super::State;
use crate::execution::CommandExecutionConfig;

/// Represents the shared state or contract used by managed command execution.
pub(in crate::execution::scheduler) struct Shared {
    /// Stores the id value used by this execution component.
    pub(in crate::execution::scheduler) id: u64,
    /// Stores the state value used by this execution component.
    pub(in crate::execution::scheduler) state: Mutex<State>,
    /// Stores the changed value used by this execution component.
    pub(in crate::execution::scheduler) changed: Condvar,
    /// Stores the config value used by this execution component.
    pub(in crate::execution::scheduler) config: CommandExecutionConfig,
}
