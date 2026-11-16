// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use qubit_operation::OperationId;

use super::CommandControl;
use super::OperationObserverSlot;
use crate::execution::contracts::RecoveryOwner;

/// Represents the recovery item state or contract used by managed command
/// execution.
pub(crate) struct RecoveryItem {
    /// Stores the owner value used by this execution component.
    pub owner: Box<dyn RecoveryOwner>,
    /// Stores the control value used by this execution component.
    pub control: Arc<CommandControl>,
    /// Stores the id value used by this execution component.
    pub id: OperationId,
    /// Stores the observer value used by this execution component.
    pub observer: Arc<OperationObserverSlot>,
}

/// Represents the recovery sender state or contract used by managed command
/// execution.
pub(crate) type RecoverySender = SyncSender<RecoveryItem>;
