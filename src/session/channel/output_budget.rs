// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared byte accounting for bounded managed-session output.

use std::sync::Mutex;

use super::output_budget_state::OutputBudgetState;

/// Tracks aggregate bytes observed across stdout and stderr.
pub(crate) struct OutputBudget {
    /// Maximum aggregate stdout and stderr bytes accepted.
    limit: u64,
    /// Shared observation count and termination status.
    pub(super) state: Mutex<OutputBudgetState>,
}

impl OutputBudget {
    /// Creates a budget with the specified aggregate byte limit.
    ///
    /// # Parameters
    ///
    /// * `limit` - Maximum number of output bytes accepted before termination.
    pub(crate) fn new(limit: u64) -> Self {
        Self {
            limit,
            state: Mutex::new(OutputBudgetState {
                observed: 0,
                exceeded: limit == 0,
            }),
        }
    }

    /// Reports whether observed output has reached the limit.
    pub(crate) fn exceeded(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).exceeded
    }

    /// Returns the number of output bytes counted against the budget.
    pub(crate) fn observed(&self) -> u64 {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).observed
    }

    /// Returns the configured aggregate byte limit.
    pub(crate) fn limit(&self) -> u64 {
        self.limit
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::OutputBudget;

    #[test]
    fn test_budget_accessors_recover_observations_after_state_lock_poisoning() {
        let budget = Arc::new(OutputBudget::new(10));
        let poisoner = budget.clone();
        let join = std::thread::spawn(move || {
            let mut state = poisoner.state.lock().unwrap();
            state.observed = 4;
            panic!("simulate a worker panic while updating output accounting");
        });
        assert!(join.join().is_err());

        assert_eq!(budget.limit(), 10);
        assert_eq!(budget.observed(), 4);
        assert!(!budget.exceeded());
    }
}
