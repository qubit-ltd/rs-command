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
    reader_gone: bool,
    incomplete: bool,
    broken_pipe: bool,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
}

pub(crate) struct ByteChannel {
    state: Mutex<State>,
    changed: Condvar,
}

impl ByteChannel {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                bytes: VecDeque::new(),
                writer_closed: false,
                reader_gone: false,
                incomplete: false,
                broken_pipe: false,
                read_waker: None,
                write_waker: None,
            }),
            changed: Condvar::new(),
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
        for chunk in bytes.chunks(MAX_BLOCK) {
            let mut offset = 0;
            while offset < chunk.len() {
                let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                while s.bytes.len() == CHANNEL_CAPACITY && !s.reader_gone {
                    s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
                }
                if s.reader_gone {
                    s.incomplete = true;
                    return Ok(());
                }
                let n = (CHANNEL_CAPACITY - s.bytes.len()).min(chunk.len() - offset);
                s.bytes.extend(&chunk[offset..offset + n]);
                offset += n;
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
}
