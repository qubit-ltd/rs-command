// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded admission and dispatch for managed command work.
mod bounded_scheduler;
mod internal;
mod kind;
mod scheduler_snapshot;
mod waiters;
mod worker;

pub use bounded_scheduler::BoundedScheduler;
pub use scheduler_snapshot::SchedulerSnapshot;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use super::BoundedScheduler;
    use crate::execution::CommandExecutionConfig;
    use crate::execution::Scheduler;
    struct CountWake(AtomicUsize);
    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn config() -> CommandExecutionConfig {
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            max_queued_input_bytes: 8,
            max_diagnostics: 1,
        }
    }
    #[test]
    fn test_reservation_release_wakes_registered_waiter() {
        let scheduler = BoundedScheduler::new(config()).unwrap();
        let permit = scheduler.try_reserve(2).unwrap();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        scheduler.register_waiter(Some(&waker)).unwrap();
        drop(permit);
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.snapshot().input_bytes, 0);
    }
    #[test]
    fn test_helper_lease_keeps_active_slot_after_job_permit_drops() {
        let scheduler = BoundedScheduler::new(config()).unwrap();
        let permit = scheduler.try_reserve(0).unwrap();
        let helper = permit.retain();
        drop(permit);
        assert_eq!(scheduler.snapshot().active, 1);
        assert_eq!(scheduler.snapshot().residual_helpers, 1);
        drop(helper);
        assert_eq!(scheduler.snapshot().active, 0);
        assert_eq!(scheduler.snapshot().residual_helpers, 0);
    }
}
