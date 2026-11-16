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
    DeadlineExpired,
    Closed,
}
/// Exclusive scheduler permit. Its one-shot release callback runs on drop, including
/// when the permit was never activated; moving it into `ActivatedRequest` keeps it held.
pub struct Reservation {
    pub(crate) scheduler_id: u64,
    pub(crate) token: u64,
    release: Option<Box<dyn FnOnce(u64) + Send + 'static>>,
    retain: Arc<dyn Fn(u64) -> ReservationLease + Send + Sync + 'static>,
}
impl Reservation {
    /// Creates a uniquely owned permit with its scheduler's nonblocking release hook.
    ///
    /// The release callback must be nonblocking and must not panic because it runs from `Drop`.
    pub(crate) fn new(
        scheduler_id: u64,
        token: u64,
        release: impl FnOnce(u64) + Send + 'static,
        retain: impl Fn(u64) -> ReservationLease + Send + Sync + 'static,
    ) -> Self {
        Self {
            scheduler_id,
            token,
            release: Some(Box::new(release)),
            retain: Arc::new(retain),
        }
    }

    /// Reports whether this permit was created by the given scheduler instance.
    pub(crate) fn belongs_to(&self, scheduler_id: u64) -> bool {
        self.scheduler_id == scheduler_id
    }

    /// Retains active capacity for one process/helper user beyond the permit's lifetime.
    pub(crate) fn retain(&self) -> ReservationLease {
        (self.retain)(self.token)
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

/// Additional active-capacity ownership retained by a process/helper user.
pub(crate) struct ReservationLease {
    release: Option<Box<dyn FnOnce() + Send + 'static>>,
}
impl ReservationLease {
    /// Creates a unique lease whose drop reports the user's final capacity release.
    pub(crate) fn new(release: impl FnOnce() + Send + 'static) -> Self {
        Self {
            release: Some(Box::new(release)),
        }
    }
}
impl fmt::Debug for ReservationLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReservationLease")
            .field("active", &self.release.is_some())
            .finish()
    }
}
impl Drop for ReservationLease {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
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
    /// Runs on a domain worker and receives the unique capacity permit.
    pub run: Box<dyn FnOnce(Reservation) + Send + 'static>,
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
    /// Blocks until a job and its unique permit are ready, or admission is closed and drained.
    fn wait_next(&self) -> Option<(ScheduledJob, Reservation)>;
    fn close_admission(&self);
}

#[cfg(test)]
mod tests {
    use super::{Reservation, ReservationLease};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn dropping_reservation_releases_its_token_once() {
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
    fn retained_lease_outlives_reservation() {
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
