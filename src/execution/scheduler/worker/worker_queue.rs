// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::VecDeque;

use super::Queued;

/// Represents the worker queue state or contract used by managed command
/// execution.
#[derive(Default)]
pub(super) struct WorkerQueue {
    /// Stores the jobs value used by this execution component.
    pub jobs: VecDeque<Queued>,
}
