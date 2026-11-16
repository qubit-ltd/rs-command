// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use thiserror::Error;

/// Invalid executor configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum CommandExecutorCreateError {
    /// Reports that the configured active capacity is zero.
    #[error("max_active must be greater than zero")]
    ZeroActiveCapacity,
    /// Reports that an executor worker could not be started.
    #[error("failed to start managed command worker")]
    WorkerStartFailed,
    /// Preserves the operation manager group binding failure.
    #[error("failed to bind command service operation group: {0}")]
    GroupBinding(#[doc = "The operation group binding failure."] qubit_operation::GroupBindingError),
}
