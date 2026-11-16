//! Dedicated bounded workers that retain uncertain process/helper ownership.
use crate::execution::contracts::{FactSink, RecoveryOwner};
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread;

type Cleanup = Box<dyn FnOnce() + Send + 'static>;

pub(crate) struct ReaperOwner(Option<Cleanup>);

impl std::fmt::Debug for ReaperOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReaperOwner")
            .finish_non_exhaustive()
    }
}

impl ReaperOwner {
    pub(crate) fn new(cleanup: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(cleanup)))
    }

    fn reap(mut self) {
        if let Some(cleanup) = self.0.take() {
            cleanup();
        }
    }

    pub(crate) fn run_inline(self) {
        self.reap();
    }
}

#[derive(Clone)]
pub(crate) struct ReaperObserver {
    pending: std::sync::Arc<AtomicBool>,
    completed: std::sync::Arc<AtomicBool>,
    recovery_transferred: std::sync::Arc<AtomicBool>,
    completion_sender: std::sync::Arc<std::sync::Mutex<Option<SyncSender<()>>>>,
    recovery_sink: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dyn FactSink>>>>,
    deferred_owner: std::sync::Arc<std::sync::Mutex<Option<Box<dyn RecoveryOwner>>>>,
}

impl ReaperObserver {
    pub(crate) fn pair() -> (Self, std::sync::mpsc::Receiver<()>) {
        let (sender, receiver) = sync_channel(1);
        (
            Self {
                pending: std::sync::Arc::new(AtomicBool::new(false)),
                completed: std::sync::Arc::new(AtomicBool::new(false)),
                recovery_transferred: std::sync::Arc::new(AtomicBool::new(false)),
                completion_sender: std::sync::Arc::new(std::sync::Mutex::new(Some(sender))),
                recovery_sink: std::sync::Arc::new(std::sync::Mutex::new(None)),
                deferred_owner: std::sync::Arc::new(std::sync::Mutex::new(None)),
            },
            receiver,
        )
    }

    pub(crate) fn pair_with_recovery_sink(
        sink: std::sync::Arc<dyn FactSink>,
    ) -> (Self, std::sync::mpsc::Receiver<()>) {
        let (observer, receiver) = Self::pair();
        *observer
            .recovery_sink
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(sink);
        (observer, receiver)
    }

    pub(crate) fn recovery_sink(&self) -> Option<std::sync::Arc<dyn FactSink>> {
        self.recovery_sink
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(crate) fn defer_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) {
        *self
            .deferred_owner
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(owner);
    }

    pub(crate) fn take_deferred_recovery_owner(&self) -> Option<Box<dyn RecoveryOwner>> {
        self.deferred_owner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }

    pub(crate) fn mark_recovery_transferred(&self) {
        self.recovery_transferred.store(true, Ordering::Release);
    }

    pub(crate) fn recovery_was_transferred(&self) -> bool {
        self.recovery_transferred.load(Ordering::Acquire)
    }

    pub(crate) fn mark_pending(&self) {
        self.pending.store(true, Ordering::Release);
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    pub(crate) fn complete(&self) {
        self.completed.store(true, Ordering::Release);
        if let Some(sender) = self
            .completion_sender
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = sender.send(());
        }
    }
}

pub(crate) struct Reaper {
    sender: SyncSender<ReaperOwner>,
    overflow_sender: SyncSender<ReaperOwner>,
}

static REAPER: OnceLock<Result<Reaper, String>> = OnceLock::new();

impl Reaper {
    fn initialize() -> io::Result<Self> {
        const WORKERS: usize = 4;
        const CAPACITY: usize = 32;
        let (sender, receiver) = sync_channel::<ReaperOwner>(CAPACITY);
        let (overflow_sender, overflow_receiver) = sync_channel::<ReaperOwner>(CAPACITY);
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
        let overflow_receiver = std::sync::Arc::new(std::sync::Mutex::new(overflow_receiver));
        for index in 0..WORKERS {
            let (receiver, name) = if index < WORKERS / 2 {
                (std::sync::Arc::clone(&receiver), "primary")
            } else {
                (std::sync::Arc::clone(&overflow_receiver), "overflow")
            };
            thread::Builder::new()
                .name(format!("qubit-command-reaper-{name}-{index}"))
                .spawn(move || {
                    loop {
                        let task = receiver
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .recv();
                        match task {
                            Ok(task) => task.reap(),
                            Err(_) => break,
                        }
                    }
                })?;
        }
        Ok(Self {
            sender,
            overflow_sender,
        })
    }

    pub(crate) fn global() -> io::Result<&'static Self> {
        match REAPER.get_or_init(|| Self::initialize().map_err(|error| error.to_string())) {
            Ok(reaper) => Ok(reaper),
            Err(error) => Err(io::Error::other(error.clone())),
        }
    }

    pub(crate) fn handoff(&self, owner: ReaperOwner) -> Result<(), ReaperOwner> {
        match self.sender.try_send(owner) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(owner) | TrySendError::Disconnected(owner)) => {
                match self.overflow_sender.try_send(owner) {
                    Ok(()) => Ok(()),
                    Err(TrySendError::Full(owner) | TrySendError::Disconnected(owner)) => {
                        Err(owner)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reaper, ReaperObserver, ReaperOwner};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    #[test]
    fn bounded_reaper_executes_transferred_owner_before_completion_signal() {
        let reaper = Reaper::global().expect("reaper workers should start");
        let released = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&released);
        let (observer, completed) = ReaperObserver::pair();
        observer.mark_pending();
        let cleanup_observer = observer.clone();
        let owner = ReaperOwner::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            cleanup_observer.complete();
        });
        reaper.handoff(owner).expect("reaper should accept owner");
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("cleanup should signal completion");
        assert_eq!(released.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn full_primary_queue_returns_owner_to_bounded_overflow_queue_without_blocking() {
        let (sender, primary) = sync_channel(1);
        let (overflow_sender, overflow) = sync_channel(1);
        let reaper = Reaper {
            sender,
            overflow_sender,
        };
        reaper
            .handoff(ReaperOwner::new(|| {}))
            .expect("primary queue should accept first owner");
        let completed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&completed);
        reaper
            .handoff(ReaperOwner::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("overflow queue should accept owner when primary is full");
        overflow
            .try_recv()
            .expect("owner should be retained by overflow queue")
            .run_inline();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        drop(primary);
    }

    #[test]
    fn both_full_queues_return_owner_for_retry_after_capacity_drains() {
        let (sender, primary) = sync_channel(1);
        let (overflow_sender, overflow) = sync_channel(1);
        let reaper = Reaper {
            sender,
            overflow_sender,
        };
        reaper.handoff(ReaperOwner::new(|| {})).unwrap();
        reaper.handoff(ReaperOwner::new(|| {})).unwrap();

        let completed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&completed);
        let owner = reaper
            .handoff(ReaperOwner::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            }))
            .expect_err("full bounded queues must return the still-owned task");
        assert_eq!(completed.load(Ordering::SeqCst), 0);

        primary
            .try_recv()
            .expect("the test should be able to release primary capacity")
            .run_inline();
        reaper
            .handoff(owner)
            .expect("returned owner should transfer after capacity drains");
        primary
            .try_recv()
            .expect("retried owner should be retained in a bounded queue")
            .run_inline();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }
}
