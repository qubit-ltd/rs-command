// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;

use super::CapacityFailure;
use super::Reservation;
use super::ScheduledJob;

/// Rejection returns both job and permit so the caller can recover ownership.
pub struct EnqueueFailure {
    /// Stores the reason value used by this execution component.
    pub reason: CapacityFailure,
    /// Stores the job value used by this execution component.
    pub job: ScheduledJob,
    /// Stores the permit value used by this execution component.
    pub permit: Reservation,
}
impl fmt::Debug for EnqueueFailure {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `f`: Supplies the f value used by this operation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnqueueFailure")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    use super::EnqueueFailure;
    use crate::execution::contracts::CapacityFailure;
    use crate::execution::contracts::Reservation;
    use crate::execution::contracts::ReservationLease;
    use crate::execution::contracts::ScheduledJob;
    use crate::execution::contracts::StopPort;

    struct NoopStop;

    impl StopPort for NoopStop {
        fn request_stop(&self, _reason: qubit_operation::StopReason) {}
    }

    #[test]
    fn test_debug_summarizes_rejection_while_failure_returns_job_and_permit() {
        let executed = Arc::new(AtomicBool::new(false));
        let callback_executed = executed.clone();
        let job = ScheduledJob {
            run: Box::new(move |_permit| {
                callback_executed.store(true, Ordering::SeqCst);
            }),
            stop: Arc::new(NoopStop),
        };
        let permit = Reservation::new(7, 41, |_| {}, |_| ReservationLease::new(|| {}));
        let failure = EnqueueFailure {
            reason: CapacityFailure::QueueFull,
            job,
            permit,
        };

        let debug = format!("{failure:?}");
        assert!(debug.contains("QueueFull"));
        assert!(!debug.contains("callback_executed"));
        assert!(!debug.contains("scheduler_id"));

        let EnqueueFailure {
            reason,
            job,
            permit,
        } = failure;
        assert_eq!(reason, CapacityFailure::QueueFull);
        job.stop
            .request_stop(qubit_operation::StopReason::UserCancelled);
        (job.run)(permit);
        assert!(executed.load(Ordering::SeqCst));
    }
}
