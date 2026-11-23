// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal types and worker functions for the retry timer.
pub(super) mod retry_delay;
pub(super) mod retry_timer;
pub(super) mod retry_timer_failure;
pub(super) mod retry_timer_message;
pub(super) mod retry_timer_permit;
pub(super) mod retry_timer_worker;

#[cfg(test)]
mod tests;
