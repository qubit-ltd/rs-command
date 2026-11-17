// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::RecoveryOwner;
use super::RecoveryPublishError;

/// Rejected transfer that returns ownership to the backend for retry or reaper
/// handoff.
pub struct RecoveryPublishFailure {
    /// Stores the reason value used by this execution component.
    pub reason: RecoveryPublishError,
    /// Stores the owner value used by this execution component.
    pub owner: Box<dyn RecoveryOwner>,
}

impl std::fmt::Debug for RecoveryPublishFailure {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `formatter`: Supplies the formatter value used by this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryPublishFailure")
            .field("reason", &self.reason)
            .field("owner_summary", &self.owner.summary())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::RecoveryPublishFailure;
    use crate::execution::contracts::RecoveryOwner;
    use crate::execution::contracts::RecoveryPublishError;
    use crate::execution::contracts::RecoveryRetryError;

    struct RetainedOwner(Arc<AtomicUsize>);

    impl RecoveryOwner for RetainedOwner {
        fn summary(&self) -> String {
            "cleanup owner remains retryable".to_owned()
        }

        fn retry_cleanup(&self) -> Result<(), RecoveryRetryError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn test_debug_describes_publish_rejection_and_owner_remains_recoverable() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let failure = RecoveryPublishFailure {
            reason: RecoveryPublishError::CapacityExceeded,
            owner: Box::new(RetainedOwner(attempts.clone())),
        };

        let debug = format!("{failure:?}");
        assert!(debug.contains("CapacityExceeded"));
        assert!(debug.contains("cleanup owner remains retryable"));
        assert!(!failure.owner.is_complete());
        failure.owner.retry_cleanup().unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
