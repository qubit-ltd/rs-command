// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::EffectRecord;

/// Bounded effect evidence carried by a recovery owner into external
/// settlement.
#[derive(Clone, Debug, Default)]
pub struct RecoveryEffectEvidence {
    /// Retained effects with their real operation and resource identities.
    pub effects: Vec<EffectRecord>,
    /// Total observed effect records before this bounded list was retained.
    pub effects_total: usize,
    /// Records omitted because the owner-specific evidence bound was reached.
    pub effect_overflow: usize,
}
