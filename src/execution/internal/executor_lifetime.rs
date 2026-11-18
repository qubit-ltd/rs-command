// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Last-handle ownership for the executor's scheduler admission lifecycle.

use std::sync::Arc;

use crate::execution::Scheduler;
use crate::execution::scheduler::BoundedScheduler;

/// Closes scheduler admission when the last external executor handle is
/// dropped.
pub(in crate::execution) struct ExecutorLifetime {
    /// Keeps the scheduler available until external executor ownership ends.
    scheduler: Arc<BoundedScheduler>,
}

impl ExecutorLifetime {
    /// Creates the shared owner associated with executor clones.
    ///
    /// # Parameters
    /// * `scheduler` - Scheduler whose admission remains open while this owner
    ///   exists.
    pub(in crate::execution) fn new(scheduler: Arc<BoundedScheduler>) -> Self {
        Self { scheduler }
    }
}

impl Drop for ExecutorLifetime {
    /// Closes admission and wakes idle workers without waiting for them to
    /// exit.
    fn drop(&mut self) {
        Scheduler::close_admission(self.scheduler.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::ExecutorLifetime;
    use crate::CommandExecutionConfig;
    use crate::execution::Scheduler;
    use crate::execution::scheduler::BoundedScheduler;

    #[test]
    fn test_last_executor_lifetime_drop_closes_and_releases_idle_worker() {
        let scheduler = Arc::new(BoundedScheduler::new(CommandExecutionConfig::default()).unwrap());
        let lifetime = ExecutorLifetime::new(Arc::clone(&scheduler));
        let worker_scheduler = Arc::clone(&scheduler);
        let (exit_tx, exit_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let exited = Scheduler::wait_next(worker_scheduler.as_ref()).is_none();
            exit_tx.send(exited).unwrap();
        });

        drop(lifetime);

        let exited = exit_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or(false);
        if !exited {
            Scheduler::close_admission(scheduler.as_ref());
        }
        worker.join().unwrap();
        assert!(exited, "dropping the final owner should release the idle worker");
        assert!(scheduler.try_reserve(0).is_err());
    }

    #[test]
    fn test_non_last_executor_lifetime_drop_keeps_admission_open() {
        let scheduler = Arc::new(BoundedScheduler::new(CommandExecutionConfig::default()).unwrap());
        let lifetime = Arc::new(ExecutorLifetime::new(Arc::clone(&scheduler)));
        let clone = Arc::clone(&lifetime);

        drop(lifetime);

        let permit = scheduler.try_reserve(0).expect("another executor clone remains");
        drop(permit);
        drop(clone);
        assert!(scheduler.try_reserve(0).is_err());
    }
}
