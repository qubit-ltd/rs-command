// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::AsyncOperationHandle;

use crate::CommandError;
use crate::CommandOutput;

/// Asynchronous command operation handle.
#[cfg(feature = "async")]
pub type AsyncCommandOperation = AsyncOperationHandle<CommandOutput, CommandError>;
