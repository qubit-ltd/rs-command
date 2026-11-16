use std::collections::VecDeque;

use crate::execution::contracts::ScheduledJob;

pub(super) struct Queued {
    pub job: ScheduledJob,
    pub permit: crate::execution::Reservation,
    pub timeout_requested: bool,
    pub timeout_pending: bool,
}
#[derive(Default)]
pub(super) struct WorkerQueue {
    pub jobs: VecDeque<Queued>,
}
