// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::ReaperObserver;
use super::ReaperOwner;
use crate::execution::backend::Reaper;
use crate::execution::contracts::RecoveryEffectEvidence;
use crate::execution::contracts::RecoveryOwner;

/// Represents the deferred reaper owner state or contract used by managed
/// command execution.
pub(crate) struct DeferredReaperOwner {
    /// Stores the owner value used by this execution component.
    owner: std::sync::Mutex<Option<ReaperOwner>>,
    /// Stores the observer value used by this execution component.
    observer: ReaperObserver,
}

impl DeferredReaperOwner {
    /// Stores the new value used by this execution component.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    /// * `observer`: Supplies the observer value used by this operation.
    pub(crate) fn new(owner: ReaperOwner, observer: ReaperObserver) -> Self {
        Self {
            owner: std::sync::Mutex::new(Some(owner)),
            observer,
        }
    }
}

impl RecoveryOwner for DeferredReaperOwner {
    /// Performs the summary step in the managed command execution lifecycle.
    fn summary(&self) -> String {
        "managed child and I/O helpers awaiting bounded reaper capacity".to_owned()
    }

    /// Performs the effect evidence step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `operation_id`: Supplies the operation id value used by this
    ///   operation.
    fn effect_evidence(&self, operation_id: qubit_operation::OperationId) -> RecoveryEffectEvidence {
        if self.observer.is_complete() || !self.observer.is_pending() {
            return RecoveryEffectEvidence::default();
        }
        let queued = self.owner.lock().unwrap_or_else(|error| error.into_inner()).is_none();
        let description = if queued {
            "managed process/helper cleanup is pending in reaper"
        } else {
            "managed process/helper cleanup awaits reaper handoff"
        };
        RecoveryEffectEvidence {
            effects: vec![qubit_operation::EffectRecord {
                operation_id,
                resource_id: None,
                fact: qubit_operation::EffectFact::Unknown,
                description: qubit_operation::DiagnosticSummary::new(description),
            }],
            effects_total: 1,
            effect_overflow: 0,
        }
    }

    /// Stores the retry cleanup value used by this execution component.
    fn retry_cleanup(&self) -> Result<(), crate::execution::contracts::RecoveryRetryError> {
        let mut slot = self.owner.lock().unwrap_or_else(|error| error.into_inner());
        let Some(owner) = slot.take() else {
            return if self.observer.is_complete() {
                Ok(())
            } else {
                Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
            };
        };
        let result = match Reaper::global() {
            Ok(reaper) => reaper.handoff(owner),
            Err(_) => Err(owner),
        };
        match result {
            Ok(()) => Ok(()),
            Err(owner) => {
                *slot = Some(owner);
                Err(crate::execution::contracts::RecoveryRetryError::CleanupHandoffUnavailable)
            }
        }
    }

    /// Performs the is complete step in the managed command execution
    /// lifecycle.
    fn is_complete(&self) -> bool {
        self.observer.is_complete()
    }
}
