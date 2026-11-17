// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::sync::Arc;

use super::Reservation;
use super::StopPort;

/// Domain task scheduled without borrowing an operation scope.
pub struct ScheduledJob {
    /// Runs on a domain worker and receives the unique capacity permit.
    pub run: Box<dyn FnOnce(Reservation) + Send + 'static>,
    /// Stores the stop value used by this execution component.
    pub stop: Arc<dyn StopPort>,
}
impl fmt::Debug for ScheduledJob {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `f`: Supplies the f value used by this operation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduledJob").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    use super::ScheduledJob;
    use crate::execution::contracts::Reservation;
    use crate::execution::contracts::ReservationLease;
    use crate::execution::contracts::StopPort;

    struct NoopStop;

    impl StopPort for NoopStop {
        fn request_stop(&self, _reason: qubit_operation::StopReason) {}
    }

    #[test]
    fn test_debug_hides_scheduled_callback_and_stop_implementation() {
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = invoked.clone();
        let job = ScheduledJob {
            run: Box::new(move |_permit| {
                callback_invoked.store(true, Ordering::SeqCst);
            }),
            stop: Arc::new(NoopStop),
        };

        assert_eq!(format!("{job:?}"), "ScheduledJob { .. }");
        assert!(!invoked.load(Ordering::SeqCst));

        let permit = Reservation::new(1, 2, |_| {}, |_| ReservationLease::new(|| {}));
        (job.run)(permit);
        assert!(invoked.load(Ordering::SeqCst));
    }
}
