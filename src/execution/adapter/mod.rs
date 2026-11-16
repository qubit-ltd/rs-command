// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bridge between command scheduling and the operation manager lifecycle.
pub(crate) mod cleanup;
mod command_control;
mod diagnostics;
mod internal;
mod operation_observer_slot;
mod recovery_item;
mod state;

pub(crate) use command_control::CommandControl;
pub(crate) use operation_observer_slot::OperationObserverSlot;
pub(crate) use recovery_item::RecoveryItem;
pub(crate) use recovery_item::RecoverySender;
pub(crate) use state::AdapterState;
#[cfg(feature = "async")]
pub(crate) use state::SessionOwners;
