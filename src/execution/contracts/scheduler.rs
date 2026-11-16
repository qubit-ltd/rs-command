//! Scheduler capacity and transfer contracts.
use qubit_operation::StopReason;
use std::fmt;
use std::sync::Arc;
/// Reason a reservation could not be obtained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityFailure {
    ActiveFull,
    QueueFull,
    InputBytesExceeded,
    Closed,
}
/// Exclusive scheduler permit. Its one-shot release callback runs on drop, including
/// when the permit was never activated; moving it into `ActivatedRequest` keeps it held.
pub struct Reservation {
    pub(crate) scheduler_id: u64,
    pub(crate) token: u64,
    release: Option<Box<dyn FnOnce(u64) + Send + 'static>>,
}
impl Reservation {
    /// Creates a uniquely owned permit with its scheduler's nonblocking release hook.
    ///
    /// The release callback must be nonblocking and must not panic because it runs from `Drop`.
    pub(crate) fn new(
        scheduler_id: u64,
        token: u64,
        release: impl FnOnce(u64) + Send + 'static,
    ) -> Self {
        Self {
            scheduler_id,
            token,
            release: Some(Box::new(release)),
        }
    }

    /// Reports whether this permit was created by the given scheduler instance.
    pub(crate) fn belongs_to(&self, scheduler_id: u64) -> bool {
        self.scheduler_id == scheduler_id
    }
}
impl fmt::Debug for Reservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Reservation")
            .field("scheduler_id", &self.scheduler_id)
            .field("token", &self.token)
            .field("active", &self.release.is_some())
            .finish()
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release(self.token);
        }
    }
}
/// Nonblocking stop-request port held by scheduled domain work.
pub trait StopPort: Send + Sync {
    fn request_stop(&self, reason: StopReason);
}

/// Domain task scheduled without borrowing an operation scope.
pub struct ScheduledJob {
    pub run: Box<dyn FnOnce() + Send + 'static>,
    pub stop: Arc<dyn StopPort>,
}
impl fmt::Debug for ScheduledJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduledJob").finish_non_exhaustive()
    }
}
/// Rejection returns both job and permit so the caller can recover ownership.
pub struct EnqueueFailure {
    pub reason: CapacityFailure,
    pub job: ScheduledJob,
    pub permit: Reservation,
}
impl fmt::Debug for EnqueueFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnqueueFailure")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}
/// Port for bounded reservations and job enqueue.
pub trait Scheduler: Send + Sync {
    fn try_reserve(&self, input_bytes: usize) -> Result<Reservation, CapacityFailure>;
    fn enqueue(&self, job: ScheduledJob, permit: Reservation) -> Result<(), EnqueueFailure>;
    fn close_admission(&self);
}

#[cfg(test)]
mod tests {
    use super::Reservation;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn dropping_reservation_releases_its_token_once() {
        let releases = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&releases);
        let permit = Reservation::new(7, 41, move |token| {
            assert_eq!(token, 41);
            observed.fetch_add(1, Ordering::SeqCst);
        });
        assert!(permit.belongs_to(7));
        assert!(!permit.belongs_to(8));
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        let activated_owner = permit;
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        drop(activated_owner);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }
}
