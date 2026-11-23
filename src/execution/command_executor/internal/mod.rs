// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Private types that own executor wait registrations.

// Owns one async capacity registration across repeated polls.
#[path = "capacity_wait.rs"]
pub(in crate::execution) mod capacity_wait;
// Owns a waiter registration for one async busy retry phase.
#[path = "scheduler_waiter_guard.rs"]
pub(in crate::execution) mod scheduler_waiter_guard;
