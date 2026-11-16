// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Process backend contract.
use super::ActivatedRequest;
use super::FactSink;
/// Executes an activated command on a domain worker and publishes lifecycle
/// facts.
pub trait Backend: Send + Sync {
    /// Stores the run value used by this execution component.
    ///
    /// # Parameters
    /// * `request`: Supplies the request value used by this operation.
    /// * `sink`: Supplies the sink value used by this operation.
    fn run(&self, request: ActivatedRequest, sink: std::sync::Arc<dyn FactSink>);
}
