// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Instant;

use super::RecoveryItem;
use crate::execution::contracts::RecoveryEffectEvidence;

/// One accepted owner retained between bounded recovery attempts.
pub(crate) struct PendingRecovery {
    /// Recovery owner and operation state retained until upstream settlement.
    pub(super) item: RecoveryItem,
    /// Effect evidence captured before cleanup handoff.
    pub(super) effect_evidence: RecoveryEffectEvidence,
    /// Earliest time at which the next cleanup or settlement observation runs.
    pub(super) next_attempt: Instant,
    /// Delay used for bounded exponential retry backoff.
    pub(super) backoff: std::time::Duration,
    /// Records that cleanup ownership has been transferred and published.
    pub(super) handed_off: bool,
}

impl PendingRecovery {
    /// Creates an accepted recovery item ready for its first attempt.
    ///
    /// # Parameters
    /// * `item`: Owner and operation state accepted by the recovery worker.
    /// * `effect_evidence`: Evidence retained for the recovery settlement.
    pub(super) fn new(item: RecoveryItem, effect_evidence: RecoveryEffectEvidence) -> Self {
        Self {
            item,
            effect_evidence,
            next_attempt: Instant::now(),
            backoff: std::time::Duration::from_millis(25),
            handed_off: false,
        }
    }
}
