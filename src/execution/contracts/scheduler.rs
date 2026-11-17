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
    use std::task::Waker;

    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::TimerRegistration;

    use super::Reservation;
    use crate::execution::contracts::CapacityFailure;
    use crate::execution::contracts::EnqueueFailure;
    use crate::execution::contracts::ReservationLease;
    use crate::execution::contracts::ScheduledJob;
    use crate::execution::contracts::Scheduler;
    use crate::execution::contracts::StopPort;

    struct NoopStop;

    impl StopPort for NoopStop {
        fn request_stop(&self, _reason: qubit_operation::StopReason) {}
    }

    struct CompatibilityScheduler(std::sync::atomic::AtomicUsize);

    impl Scheduler for CompatibilityScheduler {
        fn try_reserve(&self, _input_bytes: usize) -> Result<Reservation, CapacityFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Reservation::new(9, 12, |_| {}, |_| ReservationLease::new(|| {})))
        }

        fn enqueue(&self, job: ScheduledJob, permit: Reservation) -> Result<(), EnqueueFailure> {
            Err(EnqueueFailure {
                reason: CapacityFailure::QueueFull,
                job,
                permit,
            })
        }

        fn wait_next(&self) -> Option<(ScheduledJob, Reservation)> {
            None
        }

        fn close_admission(&self) {}
    }

    struct RejectingCompatibilityScheduler {
        calls: AtomicUsize,
        input_bytes: AtomicUsize,
    }

    impl Scheduler for RejectingCompatibilityScheduler {
        fn try_reserve(&self, input_bytes: usize) -> Result<Reservation, CapacityFailure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.input_bytes.store(input_bytes, Ordering::SeqCst);
            Err(CapacityFailure::QueueFull)
        }

        fn enqueue(&self, job: ScheduledJob, permit: Reservation) -> Result<(), EnqueueFailure> {
            Err(EnqueueFailure {
                reason: CapacityFailure::QueueFull,
                job,
                permit,
            })
        }

        fn wait_next(&self) -> Option<(ScheduledJob, Reservation)> {
            None
        }

        fn close_admission(&self) {}
    }

    struct FixedClock(ClockDomainId);

    impl ClockSource for FixedClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.0, 0)
        }

        fn register_deadline(&self, _deadline: MonotonicTime, _waker: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    struct NoopRegistration;

    impl TimerRegistration for NoopRegistration {
        fn cancel(self: Box<Self>) {}
    }

    #[test]
    fn test_default_context_reservation_delegates_to_legacy_scheduler_method() {
        let scheduler = CompatibilityScheduler(std::sync::atomic::AtomicUsize::new(0));

        let reservation = scheduler.try_reserve_until(23, None).unwrap();

        assert!(reservation.belongs_to(9));
        assert_eq!(scheduler.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_expired_context_rejects_before_legacy_reservation() {
        let domain = ClockDomainId::new_unique();
        let clock: Arc<dyn ClockSource> = Arc::new(FixedClock(domain));
        let context = OperationContext::new(ContextOptions {
            clock: Some(clock),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 0))),
            budget: None,
        })
        .unwrap();
        let scheduler = CompatibilityScheduler(std::sync::atomic::AtomicUsize::new(0));

        assert_eq!(
            scheduler.try_reserve_until(23, Some(context)).unwrap_err(),
            CapacityFailure::DeadlineExpired
        );
        assert_eq!(scheduler.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_context_reservation_forwards_legacy_capacity_failure() {
        let scheduler = RejectingCompatibilityScheduler {
            calls: AtomicUsize::new(0),
            input_bytes: AtomicUsize::new(0),
        };
        let context = OperationContext::new(ContextOptions::default()).unwrap();

        assert_eq!(
            scheduler.try_reserve_until(29, Some(context)).unwrap_err(),
            CapacityFailure::QueueFull
        );
        assert_eq!(scheduler.calls.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.input_bytes.load(Ordering::SeqCst), 29);
    }

    #[test]
    fn test_unexpired_context_delegates_reservation_with_original_byte_count() {
        let domain = ClockDomainId::new_unique();
        let clock: Arc<dyn ClockSource> = Arc::new(FixedClock(domain));
        let context = OperationContext::new(ContextOptions {
            clock: Some(clock),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 1))),
            budget: None,
        })
        .unwrap();
        let scheduler = CompatibilityScheduler(AtomicUsize::new(0));

        let reservation = scheduler.try_reserve_until(37, Some(context)).unwrap();

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

    #[test]
    fn test_compatibility_scheduler_forwards_jobs_and_close_methods() {
        let scheduler = CompatibilityScheduler(AtomicUsize::new(0));
        let executed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&executed);
        let job = ScheduledJob {
            run: Box::new(move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
            }),
            stop: Arc::new(NoopStop),
        };
        let permit = Reservation::new(9, 13, |_| {}, |_| ReservationLease::new(|| {}));

        let failure = scheduler
            .enqueue(job, permit)
            .expect_err("compatibility scheduler returns ownership on rejection");
        assert_eq!(failure.reason, CapacityFailure::QueueFull);
        failure
            .job
            .stop
            .request_stop(qubit_operation::StopReason::UserCancelled);
        (failure.job.run)(failure.permit);
        assert_eq!(executed.load(Ordering::SeqCst), 1);
        assert!(scheduler.wait_next().is_none());
        scheduler.close_admission();
    }

    #[test]
    fn test_rejecting_scheduler_forwards_jobs_without_changing_input_accounting() {
        let scheduler = RejectingCompatibilityScheduler {
            calls: AtomicUsize::new(0),
            input_bytes: AtomicUsize::new(17),
        };
        let job = ScheduledJob {
            run: Box::new(|_| {}),
            stop: Arc::new(NoopStop),
        };
        let permit = Reservation::new(9, 14, |_| {}, |_| ReservationLease::new(|| {}));

        let failure = scheduler
            .enqueue(job, permit)
            .expect_err("rejected enqueue must return the owned values");
        assert_eq!(failure.reason, CapacityFailure::QueueFull);
        assert_eq!(scheduler.input_bytes.load(Ordering::SeqCst), 17);
        assert!(scheduler.wait_next().is_none());
        scheduler.close_admission();
    }
}
