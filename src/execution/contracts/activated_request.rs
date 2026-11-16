// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_operation::OperationId;
use qubit_operation::OperationScope;

use super::FactSink;
use super::Request;
use super::Reservation;

/// Activated request exclusively held by a domain worker.
pub struct ActivatedRequest {
    /// Stores the operation id value used by this execution component.
    pub operation_id: OperationId,
    /// Stores the scope value used by this execution component.
    pub scope: OperationScope,
    /// Stores the request value used by this execution component.
    pub request: Request,
    /// Stores the reservation value used by this execution component.
    pub reservation: Reservation,
    /// Stores the facts value used by this execution component.
    pub facts: Arc<dyn FactSink>,
    /// Stores the session bridge value used by this execution component.
    pub session_bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
}
