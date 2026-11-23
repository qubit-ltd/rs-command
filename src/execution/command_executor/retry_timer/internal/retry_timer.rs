// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared bounded queue for asynchronous retry timer messages.
use super::retry_timer_failure::RetryTimerFailure;
use super::retry_timer_message::RetryTimerMessage;
use super::retry_timer_permit::RetryTimerPermit;

/// Schedules retry messages under one quota shared by the channel and worker.
pub(super) struct RetryTimer {
    /// Sender for retry messages queued to the worker.
    tx: std::sync::mpsc::SyncSender<RetryTimerMessage>,
    /// Shared count of queued and worker-pending messages.
    in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Maximum number of messages allowed in flight.
    capacity: usize,
}

impl RetryTimer {
    /// Creates a timer sender with the supplied total in-flight capacity.
    ///
    /// # Parameters
    /// * `tx`: Bounded channel used to send timer messages to the worker.
    /// * `capacity`: Maximum number of queued and pending messages.
    pub(super) fn new(tx: std::sync::mpsc::SyncSender<RetryTimerMessage>, capacity: usize) -> Self {
        Self {
            tx,
            in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            capacity,
        }
    }

    /// Returns the number of queued or pending messages holding a timer slot.
    #[cfg(test)]
    pub(super) fn in_flight(&self) -> usize {
        self.in_flight.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Reserves a timer slot and queues the supplied message.
    ///
    /// # Parameters
    /// * `item`: Timer message to schedule.
    pub(super) fn schedule(&self, mut item: RetryTimerMessage) -> Result<(), RetryTimerFailure> {
        let mut current = self.in_flight.load(std::sync::atomic::Ordering::Acquire);
        loop {
            if current >= self.capacity {
                return Err(RetryTimerFailure::Full);
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        item.permit = Some(RetryTimerPermit(std::sync::Arc::clone(&self.in_flight)));
        self.tx.try_send(item).map_err(|error| match error {
            std::sync::mpsc::TrySendError::Full(_) => RetryTimerFailure::Full,
            std::sync::mpsc::TrySendError::Disconnected(_) => RetryTimerFailure::Disconnected,
        })
    }
}

static RETRY_TIMER: std::sync::OnceLock<Result<RetryTimer, RetryTimerFailure>> = std::sync::OnceLock::new();

/// Returns the process-wide retry timer, starting its worker on first use.
pub(super) fn retry_timer() -> Result<&'static RetryTimer, RetryTimerFailure> {
    RETRY_TIMER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<RetryTimerMessage>(1024);
            std::thread::Builder::new()
                .name("qubit-command-admission-timer".to_owned())
                .spawn(move || super::retry_timer_worker::run_retry_timer(rx))
                .map_err(|_| RetryTimerFailure::Startup)?;
            Ok(RetryTimer::new(tx, 1024))
        })
        .as_ref()
        .map_err(|error| *error)
}
