// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private types that own executor wait registrations.

// Carries optional session and scope resources during async admission.
#[path = "async_submission_context.rs"]
pub(in crate::execution) mod async_submission_context;
// Owns one async capacity registration across repeated polls.
#[path = "capacity_wait.rs"]
pub(in crate::execution) mod capacity_wait;
// Owns a waiter registration for one async busy retry phase.
#[path = "scheduler_waiter_guard.rs"]
pub(in crate::execution) mod scheduler_waiter_guard;
// Carries the deadline and path snapshot used by a resource submission.
#[path = "resource_submit_context.rs"]
pub(in crate::execution) mod resource_submit_context;
