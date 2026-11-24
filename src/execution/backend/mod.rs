// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Contract namespace; behavior is implemented by the assigned module task.
mod internal;
mod process_backend;
mod reaper;
mod start_gate;

pub(crate) use internal::HandoffFailure;
pub(crate) use internal::HandoffFailureKind;
pub(crate) use internal::ReaperObserver;
pub(crate) use internal::ReaperOwner;
pub(crate) use internal::retry_cleanup_on_worker;
pub(crate) use process_backend::ProcessBackend;
pub(crate) use reaper::Reaper;
pub(crate) use start_gate::StartGate;
