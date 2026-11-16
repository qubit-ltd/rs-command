use crate::session::contracts::SessionStreamPolicy;
use std::{
    collections::VecDeque,
    io,
    sync::{Condvar, Mutex},
    task::Waker,
};

pub(crate) const CHANNEL_CAPACITY: usize = 64 * 1024;
pub(crate) const MAX_BLOCK: usize = 16 * 1024;

struct State {
    bytes: VecDeque<u8>,
    writer_closed: bool,
    stop_writes: bool,
    reader_gone: bool,
    incomplete: bool,
    broken_pipe: bool,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    policy: SessionStreamPolicy,
    budget: Option<std::sync::Arc<OutputBudget>>,
}

struct BudgetState {
    observed: u64,
    exceeded: bool,
}

pub(crate) struct OutputBudget {
    limit: u64,
    state: Mutex<BudgetState>,
}

impl OutputBudget {
    pub(crate) fn new(limit: u64) -> Self {
        Self {
            limit,
            state: Mutex::new(BudgetState {
                observed: 0,
                exceeded: limit == 0,
            }),
        }
    }

    pub(crate) fn exceeded(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exceeded
    }

    pub(crate) fn observed(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observed
    }

    pub(crate) fn limit(&self) -> u64 {
        self.limit
    }
}

pub(crate) struct ByteChannel {
    state: Mutex<State>,
    changed: Condvar,
    #[cfg(test)]
    wait_entered: (Mutex<usize>, Condvar),
}

impl ByteChannel {
    pub(crate) fn new() -> Self {
        Self::with_policy(SessionStreamPolicy::Backpressure, None)
    }

    pub(crate) fn with_policy(
        policy: SessionStreamPolicy,
        budget: Option<std::sync::Arc<OutputBudget>>,
    ) -> Self {
        Self {
            state: Mutex::new(State {
                bytes: VecDeque::new(),
                writer_closed: false,
                stop_writes: false,
                reader_gone: false,
                incomplete: false,
                broken_pipe: false,
                read_waker: None,
                write_waker: None,
                policy,
                budget,
            }),
            changed: Condvar::new(),
            #[cfg(test)]
            wait_entered: (Mutex::new(0), Condvar::new()),
        }
    }

    #[cfg(test)]
    fn note_wait(&self) {
        let (lock, condition) = &self.wait_entered;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        condition.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn wait_until_blocked(&self) {
        let (lock, condition) = &self.wait_entered;
        let mut count = lock.lock().unwrap_or_else(|e| e.into_inner());
        while *count == 0 {
            count = condition.wait(count).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(crate) fn poll_write(
        &self,
        cx: &mut std::task::Context<'_>,
        input: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        use std::task::Poll;
        if input.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if input.len() > MAX_BLOCK {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stream block exceeds 16 KiB",
            )));
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.reader_gone {
            s.broken_pipe = true;
            s.incomplete = true;
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream reader was dropped",
            )));
        }
        if s.writer_closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream is closed",
            )));
        }
        if s.stop_writes {
            s.incomplete = true;
            if let Some(budget) = s.budget.clone() {
                let mut b = budget.state.lock().unwrap_or_else(|e| e.into_inner());
                b.observed = b.observed.saturating_add(input.len() as u64);
                if b.observed >= budget.limit {
                    b.exceeded = true;
                }
            }
            return Poll::Ready(Ok(input.len()));
        }
        if s.policy == SessionStreamPolicy::Discard {
            let n = input.len().min(CHANNEL_CAPACITY - s.bytes.len());
            s.bytes.extend(&input[..n]);
            if n < input.len() {
                s.incomplete = true;
            }
            let waker = s.read_waker.take();
            self.changed.notify_all();
            drop(s);
            wake(waker);
            return Poll::Ready(Ok(input.len()));
        }
        if let Some(budget) = s.budget.clone() {
            let mut b = budget.state.lock().unwrap_or_else(|e| e.into_inner());
            if b.exceeded {
                b.observed = b.observed.saturating_add(input.len() as u64);
                s.incomplete = true;
                return Poll::Ready(Ok(input.len()));
            }
            let remaining = budget
                .limit
                .saturating_sub(b.observed)
                .min(usize::MAX as u64) as usize;
            let available = CHANNEL_CAPACITY - s.bytes.len();
            let n = input.len().min(remaining).min(available);
            s.bytes.extend(&input[..n]);
            b.observed = b.observed.saturating_add(input.len() as u64);
            let hit_limit = b.observed >= budget.limit;
            if hit_limit {
                b.exceeded = true;
            }
            if input.len() > n {
                s.incomplete = true;
            }
            let waker = s.read_waker.take();
            self.changed.notify_all();
            drop(b);
            drop(s);
            wake(waker);
            return Poll::Ready(Ok(input.len()));
        }
        let available = CHANNEL_CAPACITY - s.bytes.len();
        if available == 0 {
            register(&mut s.write_waker, cx.waker());
            // Recheck after registration while holding the same lock as consumers.
            if s.bytes.len() == CHANNEL_CAPACITY && !s.reader_gone && !s.writer_closed {
                return Poll::Pending;
            }
        }
        let n = input.len().min(available.max(1));
        s.bytes.extend(&input[..n]);
        let waker = s.read_waker.take();
        self.changed.notify_all();
        drop(s);
        wake(waker);
        Poll::Ready(Ok(n))
    }

    pub(crate) fn poll_read(
        &self,
        cx: &mut std::task::Context<'_>,
        output: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        use std::task::Poll;
        if output.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.bytes.is_empty() {
            let n = output.len().min(MAX_BLOCK).min(s.bytes.len());
            for slot in &mut output[..n] {
                *slot = s.bytes.pop_front().expect("length checked");
            }
            let waker = s.write_waker.take();
            self.changed.notify_all();
            drop(s);
            wake(waker);
            return Poll::Ready(Ok(n));
        }
        if s.writer_closed {
            return Poll::Ready(Ok(0));
        }
        register(&mut s.read_waker, cx.waker());
        if s.bytes.is_empty() && !s.writer_closed {
            Poll::Pending
        } else {
            Poll::Ready(Ok(0))
        }
    }

    pub(crate) fn close_writer(&self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.writer_closed = true;
        let waker = s.read_waker.take();
        self.changed.notify_all();
        drop(s);
        wake(waker);
    }

    pub(crate) fn drop_reader(&self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.reader_gone = true;
        s.incomplete = !s.bytes.is_empty();
        s.bytes.clear();
        let waker = s.write_waker.take();
        self.changed.notify_all();
        drop(s);
        wake(waker);
    }

    pub(crate) fn stop_output(&self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.stop_writes = true;
        s.writer_closed = true;
        s.incomplete = true;
        let writer_waker = s.write_waker.take();
        let reader_waker = s.read_waker.take();
        self.changed.notify_all();
        drop(s);
        wake(writer_waker);
        wake(reader_waker);
    }

    pub(crate) fn incomplete(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .incomplete
    }
    pub(crate) fn broken_pipe(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .broken_pipe
    }

    pub(crate) fn record_broken_pipe(&self) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .broken_pipe = true;
    }

    pub(crate) fn blocking_read(&self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while s.bytes.is_empty() && !s.writer_closed && !s.reader_gone {
            s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        if s.reader_gone {
            return Ok(0);
        }
        let n = out.len().min(MAX_BLOCK).min(s.bytes.len());
        for slot in &mut out[..n] {
            *slot = s.bytes.pop_front().expect("length checked");
        }
        let waker = s.write_waker.take();
        self.changed.notify_all();
        drop(s);
        wake(waker);
        Ok(n)
    }

    pub(crate) fn blocking_write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut total_offset = 0;
        for chunk in bytes.chunks(MAX_BLOCK) {
            let mut offset = 0;
            while offset < chunk.len() {
                let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if s.stop_writes {
                    s.incomplete = true;
                    if let Some(budget) = s.budget.clone() {
                        let mut b = budget.state.lock().unwrap_or_else(|e| e.into_inner());
                        b.observed = b
                            .observed
                            .saturating_add((bytes.len() - total_offset) as u64);
                        if b.observed >= budget.limit {
                            b.exceeded = true;
                        }
                    }
                    return Ok(());
                }
                if s.reader_gone {
                    s.incomplete = true;
                    if let Some(budget) = s.budget.clone() {
                        let mut b = budget.state.lock().unwrap_or_else(|e| e.into_inner());
                        b.observed = b
                            .observed
                            .saturating_add((bytes.len() - total_offset) as u64);
                        if b.observed >= budget.limit {
                            b.exceeded = true;
                        }
                    }
                    return Ok(());
                }
                if s.policy == SessionStreamPolicy::Discard {
                    let remaining = chunk.len() - offset;
                    let n = (CHANNEL_CAPACITY - s.bytes.len()).min(remaining);
                    s.bytes.extend(&chunk[offset..offset + n]);
                    offset += remaining;
                    total_offset += remaining;
                    if n < remaining {
                        s.incomplete = true;
                    }
                    let waker = s.read_waker.take();
                    self.changed.notify_all();
                    drop(s);
                    wake(waker);
                    continue;
                }
                if let Some(budget) = s.budget.clone() {
                    let mut b = budget.state.lock().unwrap_or_else(|e| e.into_inner());
                    if b.exceeded {
                        b.exceeded = true;
                        b.observed = b
                            .observed
                            .saturating_add((bytes.len() - total_offset) as u64);
                        s.incomplete = true;
                        return Ok(());
                    }
                    let remaining = budget
                        .limit
                        .saturating_sub(b.observed)
                        .min(usize::MAX as u64) as usize;
                    let offered = chunk.len() - offset;
                    let n = (CHANNEL_CAPACITY - s.bytes.len())
                        .min(offered)
                        .min(remaining);
                    s.bytes.extend(&chunk[offset..offset + n]);
                    offset += offered;
                    total_offset += offered;
                    b.observed = b.observed.saturating_add(offered as u64);
                    if n < offered {
                        s.incomplete = true;
                    }
                    if b.observed >= budget.limit {
                        b.exceeded = true;
                    }
                    let waker = s.read_waker.take();
                    self.changed.notify_all();
                    drop(b);
                    drop(s);
                    wake(waker);
                    continue;
                }
                while s.bytes.len() == CHANNEL_CAPACITY && !s.reader_gone && !s.stop_writes {
                    #[cfg(test)]
                    self.note_wait();
                    s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
                }
                if s.stop_writes {
                    s.incomplete = true;
                    return Ok(());
                }
                if s.reader_gone {
                    s.incomplete = true;
                    return Ok(());
                }
                let n = (CHANNEL_CAPACITY - s.bytes.len()).min(chunk.len() - offset);
                s.bytes.extend(&chunk[offset..offset + n]);
                offset += n;
                total_offset += n;
                let waker = s.read_waker.take();
                self.changed.notify_all();
                drop(s);
                wake(waker);
            }
        }
        Ok(())
    }
}

fn register(slot: &mut Option<Waker>, w: &Waker) {
    if slot.as_ref().is_none_or(|old| !old.will_wake(w)) {
        *slot = Some(w.clone());
    }
}
fn wake(w: Option<Waker>) {
    if let Some(w) = w {
        w.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{
        channel::SessionChannels,
        contracts::{SessionBridge, SessionStreamPolicy},
    };
    use qubit_io::AsyncInput;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct CountWake(AtomicUsize);
    impl std::task::Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn capacity_blocking_and_writer_wake() {
        let c = ByteChannel::new();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut context = std::task::Context::from_waker(&waker);
        let block = vec![9; MAX_BLOCK];
        for _ in 0..CHANNEL_CAPACITY / MAX_BLOCK {
            assert!(matches!(
                c.poll_write(&mut context, &block),
                std::task::Poll::Ready(Ok(MAX_BLOCK))
            ));
        }
        assert!(matches!(
            c.poll_write(&mut context, &block),
            std::task::Poll::Pending
        ));
        let mut out = [0; MAX_BLOCK];
        assert!(matches!(
            c.poll_read(&mut context, &mut out),
            std::task::Poll::Ready(Ok(MAX_BLOCK))
        ));
        assert!(counter.0.load(Ordering::SeqCst) > 0);
        assert!(matches!(
            c.poll_write(&mut context, &block),
            std::task::Poll::Ready(Ok(MAX_BLOCK))
        ));
    }

    #[test]
    fn block_limit_eof_drain_and_zero_length() {
        let c = ByteChannel::new();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut context = std::task::Context::from_waker(&waker);
        assert!(
            matches!(c.poll_write(&mut context, &vec![0; MAX_BLOCK + 1]), std::task::Poll::Ready(Err(ref e)) if e.kind() == io::ErrorKind::InvalidInput)
        );
        assert!(matches!(
            c.poll_read(&mut context, &mut []),
            std::task::Poll::Ready(Ok(0))
        ));
        assert!(matches!(
            c.poll_read(&mut context, &mut [0; 1]),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            c.poll_write(&mut context, b"abc"),
            std::task::Poll::Ready(Ok(3))
        ));
        c.close_writer();
        let mut out = [0; 8];
        assert!(matches!(
            c.poll_read(&mut context, &mut out),
            std::task::Poll::Ready(Ok(3))
        ));
        assert_eq!(&out[..3], b"abc");
        assert!(matches!(
            c.poll_read(&mut context, &mut out),
            std::task::Poll::Ready(Ok(0))
        ));
    }

    #[test]
    fn dropping_reader_marks_loss_and_broken_pipe() {
        let c = ByteChannel::new();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut context = std::task::Context::from_waker(&waker);
        assert!(matches!(
            c.poll_write(&mut context, b"lost bytes"),
            std::task::Poll::Ready(Ok(10))
        ));
        c.drop_reader();
        assert!(c.incomplete());
        assert!(
            matches!(c.poll_write(&mut context, b"next"), std::task::Poll::Ready(Err(ref e)) if e.kind() == io::ErrorKind::BrokenPipe)
        );
        assert!(c.broken_pipe());
    }

    #[test]
    fn pending_reader_is_woken_and_streams_are_independent() {
        let stdout = Arc::new(ByteChannel::new());
        let stderr = Arc::new(ByteChannel::new());
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut context = std::task::Context::from_waker(&waker);
        assert!(matches!(
            stdout.poll_read(&mut context, &mut [0; 2]),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            stderr.poll_write(&mut context, b"err"),
            std::task::Poll::Ready(Ok(3))
        ));
        assert_eq!(counter.0.load(Ordering::SeqCst), 0);
        assert!(matches!(
            stdout.poll_write(&mut context, b"ok"),
            std::task::Poll::Ready(Ok(2))
        ));
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn blocking_worker_and_async_endpoint_share_ordered_bytes() {
        let c = Arc::new(ByteChannel::new());
        let producer = c.clone();
        let thread = std::thread::spawn(move || producer.blocking_write(b"one-two-three").unwrap());
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut context = std::task::Context::from_waker(&waker);
        let mut received = Vec::new();
        while received.len() < 13 {
            let mut chunk = [0; 3];
            if let std::task::Poll::Ready(Ok(n)) = c.poll_read(&mut context, &mut chunk) {
                received.extend_from_slice(&chunk[..n]);
            } else {
                std::thread::yield_now();
            }
        }
        thread.join().unwrap();
        assert_eq!(received, b"one-two-three");
    }

    #[test]
    fn discard_policy_keeps_draining_and_marks_stream_incomplete() {
        let c = Arc::new(ByteChannel::with_policy(SessionStreamPolicy::Discard, None));
        let payload = vec![3; CHANNEL_CAPACITY + MAX_BLOCK];
        let producer = c.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread =
            std::thread::spawn(move || done_tx.send(producer.blocking_write(&payload)).unwrap());
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("discard must not block")
            .unwrap();
        thread.join().unwrap();
        assert!(c.incomplete());
        let mut buffer = vec![0; CHANNEL_CAPACITY];
        let mut total = 0;
        while total < buffer.len() {
            let n = c.blocking_read(&mut buffer[total..]).unwrap();
            assert_ne!(n, 0);
            total += n;
        }
        assert!(buffer.iter().all(|byte| *byte == 3));
        c.blocking_write(b"continued").unwrap();
        let mut tail = [0; 9];
        assert_eq!(c.blocking_read(&mut tail).unwrap(), 9);
        assert_eq!(&tail, b"continued");
    }

    #[test]
    fn aggregate_budget_is_shared_sticky_and_drains_after_limit() {
        let mut channels =
            SessionChannels::with_policy(SessionStreamPolicy::TerminateAfterBytes(5));
        channels.bridge.write_stdout(b"abc").unwrap();
        channels.bridge.write_stderr(b"def").unwrap();
        assert_eq!(channels.bridge.output_budget_limit(), Some(5));
        assert_eq!(channels.bridge.output_bytes_observed(), 6);
        assert!(channels.bridge.output_budget_exceeded());
        assert!(!channels.bridge.stdout_incomplete());
        assert!(channels.bridge.stderr_incomplete());

        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);
        let mut stdout = [0; 8];
        let mut stderr = [0; 8];
        assert!(matches!(
            std::pin::Pin::new(&mut channels.stdout).poll_read(&mut context, &mut stdout),
            std::task::Poll::Ready(Ok(3))
        ));
        assert!(matches!(
            std::pin::Pin::new(&mut channels.stderr).poll_read(&mut context, &mut stderr),
            std::task::Poll::Ready(Ok(2))
        ));
        assert_eq!(&stdout[..3], b"abc");
        assert_eq!(&stderr[..2], b"de");
        // Further output is discarded and reported without blocking the pipe drainer.
        channels.bridge.write_stdout(b"later").unwrap();
        assert!(channels.bridge.output_budget_exceeded());
        assert!(channels.bridge.stdout_incomplete());
        assert_eq!(channels.bridge.output_bytes_observed(), 11);
    }

    #[test]
    fn full_budgeted_channel_discards_after_threshold_without_blocking() {
        let channels = SessionChannels::with_policy(SessionStreamPolicy::TerminateAfterBytes(
            CHANNEL_CAPACITY as u64,
        ));
        let bridge = channels.bridge.clone();
        bridge.write_stdout(&vec![5; CHANNEL_CAPACITY]).unwrap();
        assert!(bridge.output_budget_exceeded());
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer = bridge.clone();
        let thread = std::thread::spawn(move || {
            let result = writer.write_stdout(b"after-limit");
            done_tx.send(result).unwrap();
        });
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("budgeted writer must drain")
            .unwrap();
        thread.join().unwrap();
        assert!(bridge.stdout_incomplete());
    }

    #[test]
    fn same_full_stream_discards_and_counts_until_aggregate_limit() {
        let channels = SessionChannels::with_policy(SessionStreamPolicy::TerminateAfterBytes(
            CHANNEL_CAPACITY as u64 + 100,
        ));
        let bridge = channels.bridge.clone();
        bridge.write_stdout(&vec![8; CHANNEL_CAPACITY + 1]).unwrap();
        assert_eq!(
            bridge.output_bytes_observed(),
            (CHANNEL_CAPACITY + 1) as u64
        );
        assert_eq!(
            bridge.output_budget_limit(),
            Some((CHANNEL_CAPACITY + 100) as u64)
        );
        assert!(!bridge.output_budget_exceeded());
        assert!(bridge.stdout_incomplete());

        bridge.write_stdout(&[9; 99]).unwrap();
        assert_eq!(
            bridge.output_bytes_observed(),
            (CHANNEL_CAPACITY + 100) as u64
        );
        assert!(bridge.output_budget_exceeded());
        bridge.write_stdout(b"after-limit").unwrap();
        assert_eq!(
            bridge.output_bytes_observed(),
            (CHANNEL_CAPACITY + 111) as u64
        );
        assert!(bridge.output_budget_exceeded());
        assert!(bridge.stdout_incomplete());
    }

    #[test]
    fn bridge_reports_stdin_broken_pipe_separately() {
        let channels = SessionChannels::new();
        drop(channels.stdin_reader);
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);
        assert!(
            matches!(channels.stdin.state.poll_write(&mut context, b"x"), std::task::Poll::Ready(Err(ref error)) if error.kind() == io::ErrorKind::BrokenPipe)
        );
        assert!(channels.bridge.stdin_broken_pipe());
    }

    #[test]
    fn bridge_records_os_side_stdin_broken_pipe() {
        let channels = SessionChannels::new();
        assert!(!channels.bridge.stdin_broken_pipe());
        channels.bridge.record_stdin_broken_pipe();
        assert!(channels.bridge.stdin_broken_pipe());
    }

    #[test]
    fn dropped_output_reader_still_trips_aggregate_budget() {
        let channels = SessionChannels::with_policy(SessionStreamPolicy::TerminateAfterBytes(10));
        let bridge = channels.bridge.clone();
        drop(channels.stdout);
        assert_eq!(bridge.output_bytes_observed(), 0);
        bridge.write_stdout(b"more-than-ten").unwrap();
        assert_eq!(bridge.output_bytes_observed(), 13);
        assert!(bridge.output_budget_exceeded());
        assert!(bridge.stdout_incomplete());
    }

    #[test]
    fn output_stop_wakes_writer_and_preserves_buffered_output() {
        use qubit_io::AsyncInput;

        let mut channels = SessionChannels::new();
        let bridge = channels.bridge.clone();
        bridge.write_stdout(&vec![0x2a; CHANNEL_CAPACITY]).unwrap();
        let writer_bridge = bridge.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            done_tx
                .send(writer_bridge.write_stdout(b"blocked write"))
                .unwrap();
        });
        bridge.wait_for_stdout_writer_for_test();

        bridge.request_output_stop();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("output stop must release a blocked bridge writer")
            .unwrap();
        writer.join().unwrap();
        assert!(bridge.stdout_incomplete());

        bridge.write_stdout(b"discard after stop").unwrap();
        bridge.write_stderr(b"stderr after stop").unwrap();
        assert!(bridge.stderr_incomplete());
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);
        let mut actual = Vec::new();
        let mut chunk = vec![0; MAX_BLOCK];
        while actual.len() < CHANNEL_CAPACITY {
            let n = match std::pin::Pin::new(&mut channels.stdout)
                .poll_read(&mut context, &mut chunk)
            {
                std::task::Poll::Ready(Ok(n)) => n,
                other => panic!("buffered output should remain readable: {other:?}"),
            };
            assert_ne!(n, 0);
            actual.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(actual, vec![0x2a; CHANNEL_CAPACITY]);
        assert!(matches!(
            std::pin::Pin::new(&mut channels.stdout).poll_read(&mut context, &mut chunk),
            std::task::Poll::Ready(Ok(0))
        ));
        assert!(bridge.stdout_incomplete());
    }

    #[test]
    fn reader_poll_and_producer_race_never_loses_wakeup() {
        use std::sync::Barrier;
        for _ in 0..64 {
            let c = Arc::new(ByteChannel::new());
            let barrier = Arc::new(Barrier::new(2));
            let producer_channel = c.clone();
            let producer_barrier = barrier.clone();
            let producer = std::thread::spawn(move || {
                let counter = Arc::new(CountWake(AtomicUsize::new(0)));
                let waker = std::task::Waker::from(counter);
                let mut context = std::task::Context::from_waker(&waker);
                producer_barrier.wait();
                assert!(matches!(
                    producer_channel.poll_write(&mut context, b"r"),
                    std::task::Poll::Ready(Ok(1))
                ));
            });
            let counter = Arc::new(CountWake(AtomicUsize::new(0)));
            let waker = std::task::Waker::from(counter.clone());
            let mut context = std::task::Context::from_waker(&waker);
            let mut output = [0; 1];
            barrier.wait();
            let first = c.poll_read(&mut context, &mut output);
            producer.join().unwrap();
            match first {
                std::task::Poll::Ready(Ok(1)) => assert_eq!(&output, b"r"),
                std::task::Poll::Pending => {
                    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
                    assert!(matches!(
                        c.poll_read(&mut context, &mut output),
                        std::task::Poll::Ready(Ok(1))
                    ));
                    assert_eq!(&output, b"r");
                }
                other => panic!("unexpected read result: {other:?}"),
            }
        }
    }
}
