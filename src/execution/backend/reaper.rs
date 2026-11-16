//! Dedicated bounded workers that retain uncertain process/helper ownership.
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
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
    completed: std::sync::Arc<std::sync::Mutex<Option<SyncSender<()>>>>,
}

impl ReaperObserver {
    pub(crate) fn pair() -> (Self, std::sync::mpsc::Receiver<()>) {
        let (sender, receiver) = sync_channel(1);
        (
            Self {
                pending: std::sync::Arc::new(AtomicBool::new(false)),
                completed: std::sync::Arc::new(std::sync::Mutex::new(Some(sender))),
            },
            receiver,
        )
    }

    pub(crate) fn mark_pending(&self) {
        self.pending.store(true, Ordering::Release);
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    pub(crate) fn complete(&self) {
        if let Some(sender) = self
            .completed
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
}

static REAPER: OnceLock<Result<Reaper, String>> = OnceLock::new();

impl Reaper {
    fn initialize() -> io::Result<Self> {
        const WORKERS: usize = 4;
        const CAPACITY: usize = 32;
        let (sender, receiver) = sync_channel::<ReaperOwner>(CAPACITY);
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
        for index in 0..WORKERS {
            let receiver = std::sync::Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("qubit-command-reaper-{index}"))
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
        Ok(Self { sender })
    }

    pub(crate) fn global() -> io::Result<&'static Self> {
        match REAPER.get_or_init(|| Self::initialize().map_err(|error| error.to_string())) {
            Ok(reaper) => Ok(reaper),
            Err(error) => Err(io::Error::other(error.clone())),
        }
    }

    pub(crate) fn handoff(&self, owner: ReaperOwner) -> Result<(), ReaperOwner> {
        match self.sender.send(owner) {
            Ok(()) => Ok(()),
            Err(error) => Err(error.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reaper, ReaperObserver, ReaperOwner};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
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
}
