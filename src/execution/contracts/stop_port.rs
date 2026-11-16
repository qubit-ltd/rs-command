// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::StopReason;

/// Nonblocking stop-request port held by scheduled domain work.
pub trait StopPort: Send + Sync {
    /// Stores the request stop value used by this execution component.
    ///
    /// # Parameters
    /// * `reason`: Supplies the reason value used by this operation.
    fn request_stop(&self, reason: StopReason);
}
