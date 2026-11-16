//! Scheduler capacity and transfer contracts.
use std::fmt;
/// Reason a reservation could not be obtained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityFailure {
    ActiveFull,
    QueueFull,
    InputBytesExceeded,
    Closed,
}
/// Exclusive permit for one reservation; dropping returns unactivated capacity.
#[derive(Debug)]
#[allow(dead_code)]
pub struct Reservation {
    pub(crate) token: u64,
}
impl Reservation {
    #[allow(dead_code)]
    pub(crate) fn new(token: u64) -> Self {
        Self { token }
    }
}
/// Domain task scheduled without borrowing an operation scope.
pub struct ScheduledJob {
    pub run: Box<dyn FnOnce() + Send + 'static>,
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
