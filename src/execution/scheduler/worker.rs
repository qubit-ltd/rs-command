use crate::execution::contracts::ScheduledJob;
use std::collections::VecDeque;

pub(super) struct Queued {
    pub job: ScheduledJob,
    pub permit: crate::execution::Reservation,
    pub timeout_requested: bool,
}
#[derive(Default)]
pub(super) struct WorkerQueue {
    pub jobs: VecDeque<Queued>,
}
