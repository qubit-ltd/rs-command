// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::collections::VecDeque;
use std::io;
use std::sync::Condvar;
use std::sync::Mutex;
use std::task::Waker;

use super::channel_state::ChannelState;
use super::output_budget::OutputBudget;
use crate::session::contracts::SessionStreamPolicy;

/// Maximum number of bytes buffered by one managed stream.
pub(crate) const CHANNEL_CAPACITY: usize = 64 * 1024;
/// Maximum number of bytes accepted or returned by one channel operation.
pub(crate) const MAX_BLOCK: usize = 16 * 1024;

/// Bounded byte queue shared by one producer and one consumer.
pub(crate) struct ByteChannel {
    /// Queue state and lifecycle flags protected from concurrent access.
    state: Mutex<ChannelState>,
    /// Wakes blocking readers and writers after queue state changes.
    changed: Condvar,
    /// Test-only signal that a writer reached the capacity wait.
    #[cfg(test)]
    wait_entered: (Mutex<usize>, Condvar),
    /// Test-only signal that a blocking reader entered the empty-queue wait.
    #[cfg(test)]
    reader_wait_entered: (Mutex<usize>, Condvar),
}

impl ByteChannel {
    /// Creates an empty channel with bounded backpressure.
    pub(crate) fn new() -> Self {
        Self::with_policy(SessionStreamPolicy::Backpressure, None)
    }

    /// Creates an empty channel using the selected output policy.
    ///
    /// # Parameters
    ///
    /// * `policy` - Behavior when the bounded output queue fills.
    /// * `budget` - Optional aggregate limit shared by related output streams.
    pub(crate) fn with_policy(policy: SessionStreamPolicy, budget: Option<std::sync::Arc<OutputBudget>>) -> Self {
        Self {
            state: Mutex::new(ChannelState {
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
            #[cfg(test)]
            reader_wait_entered: (Mutex::new(0), Condvar::new()),
        }
    }

    /// Signals test observers that a blocking writer reached its wait.
    #[cfg(test)]
    fn note_wait(&self) {
        let (lock, condition) = &self.wait_entered;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        condition.notify_all();
    }

    /// Waits until a writer reports reaching the capacity wait.
    #[cfg(test)]
    pub(crate) fn wait_until_blocked(&self) {
        let (lock, condition) = &self.wait_entered;
        let mut count = lock.lock().unwrap_or_else(|e| e.into_inner());
        while *count == 0 {
            count = condition.wait(count).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Signals test observers that a blocking reader reached its wait.
    #[cfg(test)]
    fn note_reader_wait(&self) {
        let (lock, condition) = &self.reader_wait_entered;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        condition.notify_all();
    }

    /// Waits until a blocking reader reports reaching the empty-queue wait.
    #[cfg(test)]
    fn wait_until_reader_blocked(&self) {
        let (lock, condition) = &self.reader_wait_entered;
        let mut count = lock.lock().unwrap_or_else(|e| e.into_inner());
        while *count == 0 {
            count = condition.wait(count).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Writes a bounded chunk without blocking the async executor.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used to register when capacity is unavailable.
    /// * `input` - Bytes offered to the channel, limited to `MAX_BLOCK`.
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
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "stream is closed")));
        }
        if s.stop_writes {
            s.incomplete = true;
            if let Some(budget) = s.budget.clone() {
                let mut b = budget.state.lock().unwrap_or_else(|e| e.into_inner());
                b.observed = b.observed.saturating_add(input.len() as u64);
                if b.observed >= budget.limit() {
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
            let remaining = budget.limit().saturating_sub(b.observed).min(usize::MAX as u64) as usize;
            let available = CHANNEL_CAPACITY - s.bytes.len();
            let n = input.len().min(remaining).min(available);
            s.bytes.extend(&input[..n]);
            b.observed = b.observed.saturating_add(input.len() as u64);
            let hit_limit = b.observed >= budget.limit();
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

    /// Reads currently available bytes without blocking the async executor.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used to register when no bytes are available.
    /// * `output` - Buffer receiving the next available bytes.
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

    /// Marks the producer closed and wakes a waiting consumer.
    pub(crate) fn close_writer(&self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.writer_closed = true;
        let waker = s.read_waker.take();
        self.changed.notify_all();
        drop(s);
        wake(waker);
    }

    /// Marks the consumer gone, discards queued data, and wakes producers.
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

    /// Stops future output while retaining bytes already queued.
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

    /// Reports whether bytes were discarded before delivery.
    pub(crate) fn incomplete(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).incomplete
    }
    /// Reports whether the producer observed a dropped consumer.
    pub(crate) fn broken_pipe(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).broken_pipe
    }

    /// Records a child-side broken-pipe observation.
    pub(crate) fn record_broken_pipe(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).broken_pipe = true;
    }

    /// Blocks until bytes arrive or the channel closes, then reads a chunk.
    ///
    /// # Parameters
    ///
    /// * `out` - Buffer receiving the next available bytes.
    pub(crate) fn blocking_read(&self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while s.bytes.is_empty() && !s.writer_closed && !s.reader_gone {
            #[cfg(test)]
            self.note_reader_wait();
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

    /// Writes all bytes, waiting for bounded queue capacity as needed.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Process output to publish to the session consumer.
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
                        b.observed = b.observed.saturating_add((bytes.len() - total_offset) as u64);
                        if b.observed >= budget.limit() {
                            b.exceeded = true;
                        }
                    }
                    return Ok(());
                }
                if s.reader_gone {
                    s.incomplete = true;
                    if let Some(budget) = s.budget.clone() {
                        let mut b = budget.state.lock().unwrap_or_else(|e| e.into_inner());
                        b.observed = b.observed.saturating_add((bytes.len() - total_offset) as u64);
                        if b.observed >= budget.limit() {
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
                        b.observed = b.observed.saturating_add((bytes.len() - total_offset) as u64);
                        s.incomplete = true;
                        return Ok(());
                    }
                    let remaining = budget.limit().saturating_sub(b.observed).min(usize::MAX as u64) as usize;
                    let offered = chunk.len() - offset;
                    let n = (CHANNEL_CAPACITY - s.bytes.len()).min(offered).min(remaining);
                    s.bytes.extend(&chunk[offset..offset + n]);
                    offset += offered;
                    total_offset += offered;
                    b.observed = b.observed.saturating_add(offered as u64);
                    if n < offered {
                        s.incomplete = true;
                    }
                    if b.observed >= budget.limit() {
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

/// Replaces a registered task waker when it points to another task.
///
/// # Parameters
///
/// * `slot` - Waker slot protected by the channel state lock.
/// * `w` - Task waker to register.
fn register(slot: &mut Option<Waker>, w: &Waker) {
    if slot.as_ref().is_none_or(|old| !old.will_wake(w)) {
        *slot = Some(w.clone());
    }
}
/// Wakes a task after releasing the channel state lock.
///
/// # Parameters
///
/// * `w` - Optional task waker removed from a channel state slot.
fn wake(w: Option<Waker>) {
    if let Some(w) = w {
        w.wake();
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use qubit_io::AsyncInput;

    use super::ByteChannel;
    use super::CHANNEL_CAPACITY;
    use super::MAX_BLOCK;
    use crate::session::channel::OutputBudget;
    use crate::session::channel::SessionChannels;
    use crate::session::contracts::SessionBridge;
    use crate::session::contracts::SessionStreamPolicy;
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
    fn test_capacity_blocking_and_writer_wake() {
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
        assert!(matches!(c.poll_write(&mut context, &block), std::task::Poll::Pending));
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
    fn test_block_limit_eof_drain_and_zero_length() {
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
    fn test_closed_writer_rejects_writes_and_reader_observes_eof() {
        let channel = ByteChannel::new();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);

        channel.close_writer();
        assert!(matches!(
            channel.poll_write(&mut context, b"late"),
            std::task::Poll::Ready(Err(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));
        assert!(!channel.broken_pipe());
        assert!(matches!(
            channel.poll_read(&mut context, &mut [0; 4]),
            std::task::Poll::Ready(Ok(0))
        ));
        assert_eq!(channel.blocking_read(&mut []).unwrap(), 0);
        assert_eq!(channel.blocking_read(&mut [0; 1]).unwrap(), 0);
    }

    #[test]
    fn test_discard_policy_accepts_full_write_and_tracks_truncated_buffer() {
        let channel = ByteChannel::with_policy(SessionStreamPolicy::Discard, None);
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);
        let input = vec![0x5a; CHANNEL_CAPACITY + 31];

        let mut written = 0;
        for chunk in input.chunks(MAX_BLOCK) {
            assert!(matches!(
                channel.poll_write(&mut context, chunk),
                std::task::Poll::Ready(Ok(n)) if n == chunk.len()
            ));
            written += chunk.len();
        }
        assert_eq!(written, input.len());
        assert!(channel.incomplete());
        let mut output = vec![0; CHANNEL_CAPACITY + MAX_BLOCK];
        let mut total = 0;
        while total < CHANNEL_CAPACITY {
            match channel.poll_read(&mut context, &mut output[total..]) {
                std::task::Poll::Ready(Ok(n)) => total += n,
                other => panic!("buffered discard-policy output should be readable: {other:?}"),
            }
        }
        assert_eq!(total, CHANNEL_CAPACITY);
        assert!(output[..total].iter().all(|byte| *byte == 0x5a));
    }

    #[test]
    fn test_zero_budget_discards_bytes_but_counts_observed_output() {
        let budget = Arc::new(OutputBudget::new(0));
        let channel = ByteChannel::with_policy(SessionStreamPolicy::TerminateAfterBytes(0), Some(budget.clone()));
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);

        assert!(matches!(
            channel.poll_write(&mut context, b"over limit"),
            std::task::Poll::Ready(Ok(10))
        ));
        assert!(budget.exceeded());
        assert_eq!(budget.observed(), 10);
        assert!(channel.incomplete());
        let mut output = [0; 16];
        assert!(matches!(
            channel.poll_read(&mut context, &mut output),
            std::task::Poll::Pending
        ));
    }

    #[test]
    fn test_dropping_reader_marks_loss_and_broken_pipe() {
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
    fn test_pending_reader_is_woken_and_streams_are_independent() {
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
    fn test_pending_read_and_write_replace_obsolete_wakers() {
        let channel = ByteChannel::new();
        let first = Arc::new(CountWake(AtomicUsize::new(0)));
        let second = Arc::new(CountWake(AtomicUsize::new(0)));
        let first_waker = std::task::Waker::from(first.clone());
        let second_waker = std::task::Waker::from(second.clone());
        let mut first_context = std::task::Context::from_waker(&first_waker);
        let mut second_context = std::task::Context::from_waker(&second_waker);
        let mut output = [0; 1];

        assert!(matches!(
            channel.poll_read(&mut first_context, &mut output),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            channel.poll_read(&mut second_context, &mut output),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            channel.poll_write(&mut first_context, b"r"),
            std::task::Poll::Ready(Ok(1))
        ));
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);

        let channel = ByteChannel::new();
        let block = vec![4; MAX_BLOCK];
        for _ in 0..CHANNEL_CAPACITY / MAX_BLOCK {
            assert!(matches!(
                channel.poll_write(&mut first_context, &block),
                std::task::Poll::Ready(Ok(MAX_BLOCK))
            ));
        }
        assert!(matches!(
            channel.poll_write(&mut first_context, b"blocked"),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            channel.poll_write(&mut second_context, b"blocked"),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            channel.poll_read(&mut first_context, &mut output),
            std::task::Poll::Ready(Ok(1))
        ));
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_blocking_write_after_reader_drop_reports_incomplete_output() {
        let channel = ByteChannel::new();
        channel.blocking_write(b"queued output").unwrap();
        channel.drop_reader();

        assert_eq!(channel.blocking_read(&mut [0; 8]).unwrap(), 0);
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);
        assert!(matches!(
            channel.poll_write(&mut context, b"poll after drop"),
            std::task::Poll::Ready(Err(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));
        channel.blocking_write(b"output after reader drop").unwrap();
        assert!(channel.incomplete());
        assert!(channel.broken_pipe());
    }

    #[test]
    fn test_blocking_worker_and_async_endpoint_share_ordered_bytes() {
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
    fn test_discard_policy_keeps_draining_and_marks_stream_incomplete() {
        let c = Arc::new(ByteChannel::with_policy(SessionStreamPolicy::Discard, None));
        let payload = vec![3; CHANNEL_CAPACITY + MAX_BLOCK];
        let producer = c.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || done_tx.send(producer.blocking_write(&payload)).unwrap());
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
    fn test_aggregate_budget_is_shared_sticky_and_drains_after_limit() {
        let mut channels = SessionChannels::with_policy(SessionStreamPolicy::TerminateAfterBytes(5));
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
    fn test_full_budgeted_channel_discards_after_threshold_without_blocking() {
        let channels = SessionChannels::with_policy(SessionStreamPolicy::TerminateAfterBytes(CHANNEL_CAPACITY as u64));
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
    fn test_same_full_stream_discards_and_counts_until_aggregate_limit() {
        let channels =
            SessionChannels::with_policy(SessionStreamPolicy::TerminateAfterBytes(CHANNEL_CAPACITY as u64 + 100));
        let bridge = channels.bridge.clone();
        bridge.write_stdout(&vec![8; CHANNEL_CAPACITY + 1]).unwrap();
        assert_eq!(bridge.output_bytes_observed(), (CHANNEL_CAPACITY + 1) as u64);
        assert_eq!(bridge.output_budget_limit(), Some((CHANNEL_CAPACITY + 100) as u64));
        assert!(!bridge.output_budget_exceeded());
        assert!(bridge.stdout_incomplete());

        bridge.write_stdout(&[9; 99]).unwrap();
        assert_eq!(bridge.output_bytes_observed(), (CHANNEL_CAPACITY + 100) as u64);
        assert!(bridge.output_budget_exceeded());
        bridge.write_stdout(b"after-limit").unwrap();
        assert_eq!(bridge.output_bytes_observed(), (CHANNEL_CAPACITY + 111) as u64);
        assert!(bridge.output_budget_exceeded());
        assert!(bridge.stdout_incomplete());
    }

    #[test]
    fn test_bridge_reports_stdin_broken_pipe_separately() {
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
    fn test_bridge_records_os_side_stdin_broken_pipe() {
        let channels = SessionChannels::new();
        assert!(!channels.bridge.stdin_broken_pipe());
        channels.bridge.record_stdin_broken_pipe();
        assert!(channels.bridge.stdin_broken_pipe());
    }

    #[test]
    fn test_dropped_output_reader_still_trips_aggregate_budget() {
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
    fn test_output_stop_wakes_writer_and_preserves_buffered_output() {
        use qubit_io::AsyncInput;

        let mut channels = SessionChannels::new();
        let bridge = channels.bridge.clone();
        bridge.write_stdout(&vec![0x2a; CHANNEL_CAPACITY]).unwrap();
        let writer_bridge = bridge.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            done_tx.send(writer_bridge.write_stdout(b"blocked write")).unwrap();
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
            let n = match std::pin::Pin::new(&mut channels.stdout).poll_read(&mut context, &mut chunk) {
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
    fn test_reader_poll_and_producer_race_never_loses_wakeup() {
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

    #[test]
    fn test_empty_write_and_close_wakes_pending_reader() {
        let channel = ByteChannel::new();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut context = std::task::Context::from_waker(&waker);

        assert!(matches!(
            channel.poll_write(&mut context, &[]),
            std::task::Poll::Ready(Ok(0))
        ));
        assert!(matches!(
            channel.poll_read(&mut context, &mut [0; 1]),
            std::task::Poll::Pending
        ));
        channel.close_writer();

        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert!(matches!(
            channel.poll_read(&mut context, &mut [0; 1]),
            std::task::Poll::Ready(Ok(0))
        ));
    }

    #[test]
    fn test_dropping_reader_wakes_pending_async_writer() {
        let channel = ByteChannel::new();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut context = std::task::Context::from_waker(&waker);
        let block = vec![0x5a; MAX_BLOCK];
        for _ in 0..CHANNEL_CAPACITY / MAX_BLOCK {
            assert!(matches!(
                channel.poll_write(&mut context, &block),
                std::task::Poll::Ready(Ok(MAX_BLOCK))
            ));
        }
        assert!(matches!(
            channel.poll_write(&mut context, b"wait for reader"),
            std::task::Poll::Pending
        ));

        channel.drop_reader();

        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert!(matches!(
            channel.poll_write(&mut context, b"reader is gone"),
            std::task::Poll::Ready(Err(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));
        assert!(channel.incomplete());
        assert!(channel.broken_pipe());
    }

    #[test]
    fn test_blocking_writer_resumes_after_capacity_is_read_without_reordering() {
        let channel = Arc::new(ByteChannel::new());
        let bytes = (0..=u8::MAX).cycle().take(CHANNEL_CAPACITY + 37).collect::<Vec<_>>();
        let writer_channel = channel.clone();
        let writer_bytes = bytes.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            done_tx.send(writer_channel.blocking_write(&writer_bytes)).unwrap();
        });
        channel.wait_until_blocked();

        let mut actual = Vec::with_capacity(bytes.len());
        let mut chunk = vec![0; MAX_BLOCK];
        let n = channel.blocking_read(&mut chunk).unwrap();
        assert_eq!(n, MAX_BLOCK);
        actual.extend_from_slice(&chunk[..n]);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("reading capacity should release the blocked writer")
            .unwrap();
        writer.join().unwrap();

        while actual.len() < bytes.len() {
            let n = channel.blocking_read(&mut chunk).unwrap();
            assert_ne!(n, 0, "all accepted bytes should be readable");
            actual.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(actual, bytes);
    }

    #[test]
    fn test_blocking_reader_wakes_for_data_and_writer_close() {
        let channel = Arc::new(ByteChannel::new());
        let reader_channel = channel.clone();
        let reader = std::thread::spawn(move || {
            let mut output = [0; 8];
            let count = reader_channel.blocking_read(&mut output).unwrap();
            (count, output)
        });
        channel.wait_until_reader_blocked();
        channel.blocking_write(b"arrived").unwrap();
        let (count, output) = reader.join().unwrap();
        assert_eq!(&output[..count], b"arrived");

        let closed = Arc::new(ByteChannel::new());
        let reader_channel = closed.clone();
        let reader = std::thread::spawn(move || reader_channel.blocking_read(&mut [0; 8]).unwrap());
        closed.wait_until_reader_blocked();
        closed.close_writer();
        assert_eq!(reader.join().unwrap(), 0);
    }

    #[test]
    fn test_blocking_write_after_output_stop_counts_dropped_budget_bytes() {
        let budget = Arc::new(OutputBudget::new(10));
        let channel = ByteChannel::with_policy(SessionStreamPolicy::Backpressure, Some(budget.clone()));
        channel.stop_output();

        channel.blocking_write(b"five!").unwrap();
        assert_eq!(budget.observed(), 5);
        assert!(!budget.exceeded());
        assert!(channel.incomplete());
        channel.blocking_write(b"sixsix").unwrap();
        assert_eq!(budget.observed(), 11);
        assert!(budget.exceeded());

        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);
        let mut output = [0; 1];
        assert!(matches!(
            channel.poll_read(&mut context, &mut output),
            std::task::Poll::Ready(Ok(0))
        ));
    }

    #[test]
    fn test_exact_budget_boundary_retains_bytes_then_discards_later_output() {
        let budget = Arc::new(OutputBudget::new(3));
        let channel = ByteChannel::with_policy(
            SessionStreamPolicy::TerminateAfterBytes(3),
            Some(budget.clone()),
        );
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter);
        let mut context = std::task::Context::from_waker(&waker);
        let mut output = [0; 8];

        assert!(matches!(
            channel.poll_write(&mut context, b"abc"),
            std::task::Poll::Ready(Ok(3))
        ));
        assert_eq!(budget.observed(), 3);
        assert!(budget.exceeded());
        assert!(!channel.incomplete());
        assert!(matches!(
            channel.poll_read(&mut context, &mut output),
            std::task::Poll::Ready(Ok(3))
        ));
        assert_eq!(&output[..3], b"abc");

        assert!(matches!(
            channel.poll_write(&mut context, b"d"),
            std::task::Poll::Ready(Ok(1))
        ));
        assert_eq!(budget.observed(), 4);
        assert!(channel.incomplete());
        assert!(matches!(
            channel.poll_read(&mut context, &mut output),
            std::task::Poll::Pending
        ));
    }
}
