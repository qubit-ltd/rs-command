// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::OperationContext;

use super::CapacityFailure;
use super::EnqueueFailure;
use super::Reservation;
use super::ScheduledJob;

/// Port for bounded reservations and job enqueue.
pub trait Scheduler: Send + Sync {
    /// Stores the try reserve value used by this execution component.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    fn try_reserve(&self, input_bytes: usize) -> Result<Reservation, CapacityFailure>;
    /// Reserves capacity using the context captured at the original submission.
    ///
    /// Implementations should use the inherited clock domain for admission and
    /// queued work. The default preserves compatibility for simple schedulers
    /// while still rejecting an already-expired context.
    ///
    /// # Parameters
    /// * `input_bytes`: Supplies the input bytes value used by this operation.
    /// * `context`: Supplies the context value used by this operation.
    fn try_reserve_until(
        &self,
        input_bytes: usize,
        context: Option<OperationContext>,
    ) -> Result<Reservation, CapacityFailure> {
        if context.as_ref().is_some_and(OperationContext::is_expired) {
            return Err(CapacityFailure::DeadlineExpired);
        }
        self.try_reserve(input_bytes)
    }
    /// Stores the enqueue value used by this execution component.
    ///
    /// # Parameters
    /// * `job`: Supplies the job value used by this operation.
    /// * `permit`: Supplies the permit value used by this operation.
    fn enqueue(&self, job: ScheduledJob, permit: Reservation) -> Result<(), EnqueueFailure>;
    /// Blocks until a job and its unique permit are ready, or admission is
    /// closed and drained.
    fn wait_next(&self) -> Option<(ScheduledJob, Reservation)>;
    /// Performs the close admission step in the managed command execution
    /// lifecycle.
    fn close_admission(&self);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::Reservation;
    use crate::execution::contracts::CapacityFailure;
    use crate::execution::contracts::EnqueueFailure;
    use crate::execution::contracts::ReservationLease;
    use crate::execution::contracts::ScheduledJob;
    use crate::execution::contracts::Scheduler;

    struct CompatibilityScheduler(std::sync::atomic::AtomicUsize);

    impl Scheduler for CompatibilityScheduler {
        fn try_reserve(&self, _input_bytes: usize) -> Result<Reservation, CapacityFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Reservation::new(9, 12, |_| {}, |_| ReservationLease::new(|| {})))
        }

        fn enqueue(&self, _job: ScheduledJob, _permit: Reservation) -> Result<(), EnqueueFailure> {
            unreachable!("enqueue is outside this compatibility test")
        }

        fn wait_next(&self) -> Option<(ScheduledJob, Reservation)> {
            None
        }

        fn close_admission(&self) {}
    }

    #[test]
    fn test_default_context_reservation_delegates_to_legacy_scheduler_method() {
        let scheduler = CompatibilityScheduler(std::sync::atomic::AtomicUsize::new(0));

        let reservation = scheduler.try_reserve_until(23, None).unwrap();

        assert!(reservation.belongs_to(9));
        assert_eq!(scheduler.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_dropping_reservation_releases_its_token_once() {
        let releases = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&releases);
        let permit = Reservation::new(
            7,
            41,
            move |token| {
                assert_eq!(token, 41);
                observed.fetch_add(1, Ordering::SeqCst);
            },
            |_| ReservationLease::new(|| {}),
        );
        assert!(permit.belongs_to(7));
        assert!(!permit.belongs_to(8));
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        let activated_owner = permit;
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        drop(activated_owner);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_retained_lease_outlives_reservation() {
        let releases = Arc::new(AtomicUsize::new(0));
        let owner_release = Arc::clone(&releases);
        let lease_release = Arc::clone(&releases);
        let permit = Reservation::new(
            7,
            42,
            move |_| {
                owner_release.fetch_add(1, Ordering::SeqCst);
            },
            move |_| {
                let lease_release = Arc::clone(&lease_release);
                ReservationLease::new(move || {
                    lease_release.fetch_add(1, Ordering::SeqCst);
                })
            },
        );
        let helper = permit.retain();
        drop(permit);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        drop(helper);
        assert_eq!(releases.load(Ordering::SeqCst), 2);
    }
}
