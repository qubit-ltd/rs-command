// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::RawHandle;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use super::super::super::cancel::cancel_synchronous_io;
use super::super::super::captured_output::CapturedOutput;
use super::super::super::io_cancellation::IoCancellation;
use super::super::super::output_capture_error::OutputCaptureError;
use super::super::super::output_capture_options::OutputCaptureOptions;
use super::super::super::output_tee::OutputTee;
use super::super::super::stdin_pipe::write_stdin_until_cancelled;
use super::super::super::total_output_budget::TotalOutputBudget;
use super::super::read_output_inner;
use crate::OutputStream;

const ATTEMPTS: usize = 128;
const EVENT_TIMEOUT: Duration = Duration::from_secs(2);
const BLOCKED_WRITE_OBSERVATION: Duration = Duration::from_millis(10);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreatePipe(
        read_pipe: *mut RawHandle,
        write_pipe: *mut RawHandle,
        attributes: *const c_void,
        size: u32,
    ) -> i32;
    fn GetNamedPipeInfo(
        pipe: RawHandle,
        flags: *mut u32,
        outbound_buffer_size: *mut u32,
        inbound_buffer_size: *mut u32,
        max_instances: *mut u32,
    ) -> i32;
    fn PeekNamedPipe(
        pipe: RawHandle,
        buffer: *mut c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
}

enum ProbeEvent {
    ReadinessReturned(Instant),
    ReadEntry(Instant),
    ReadReturn(Instant, Option<usize>, Option<i32>, bool),
    TeeEntry(Instant),
    TeeReturn(Instant, Option<usize>, Option<i32>),
    TeeFlushEntry(Instant),
    TeeFlushReturn(Instant, Option<i32>),
    StdinWriteEntry(Instant),
    StdinWriteReturn(Instant, Option<usize>, Option<i32>, bool),
    WorkerDone(Instant),
}

struct ReadEntry(
    File,
    mpsc::Sender<ProbeEvent>,
    Arc<std::sync::atomic::AtomicBool>,
    bool,
);

impl Read for ReadEntry {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.3 {
            let _ = self.1.send(ProbeEvent::ReadinessReturned(Instant::now()));
        }
        let _ = self.1.send(ProbeEvent::ReadEntry(Instant::now()));
        let result = self.0.read(buffer);
        let _ = self.1.send(ProbeEvent::ReadReturn(
            Instant::now(),
            result.as_ref().ok().copied(),
            result.as_ref().err().and_then(io::Error::raw_os_error),
            self.2.load(Ordering::Acquire),
        ));
        result
    }
}

struct TeeEntry(File, mpsc::Sender<ProbeEvent>);

struct StdinWriteEntry(
    File,
    mpsc::Sender<ProbeEvent>,
    Arc<std::sync::atomic::AtomicBool>,
);

impl Write for StdinWriteEntry {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let _ = self.1.send(ProbeEvent::StdinWriteEntry(Instant::now()));
        let result = self.0.write(buffer);
        let _ = self.1.send(ProbeEvent::StdinWriteReturn(
            Instant::now(),
            result.as_ref().ok().copied(),
            result.as_ref().err().and_then(io::Error::raw_os_error),
            self.2.load(Ordering::Acquire),
        ));
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Write for TeeEntry {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let _ = self.1.send(ProbeEvent::TeeEntry(Instant::now()));
        let result = self.0.write(buffer);
        let _ = self.1.send(ProbeEvent::TeeReturn(
            Instant::now(),
            result.as_ref().ok().copied(),
            result.as_ref().err().and_then(io::Error::raw_os_error),
        ));
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = self.1.send(ProbeEvent::TeeFlushEntry(Instant::now()));
        let result = self.0.flush();
        let _ = self.1.send(ProbeEvent::TeeFlushReturn(
            Instant::now(),
            result.as_ref().err().and_then(io::Error::raw_os_error),
        ));
        result
    }
}

type WorkerResult = Result<CapturedOutput, OutputCaptureError>;

struct AttemptRecovery {
    barrier: Arc<Barrier>,
    barrier_released: bool,
    source_writer: Option<thread::JoinHandle<io::Result<()>>>,
    worker: Option<thread::JoinHandle<()>>,
    tee_reader: Option<File>,
}

impl AttemptRecovery {
    fn release_barrier(&mut self) {
        if !self.barrier_released {
            self.barrier_released = true;
            self.barrier.wait();
        }
    }

    fn drain_tee(&mut self, drained: &mut Vec<u8>) -> io::Result<()> {
        self.tee_reader
            .as_mut()
            .expect("tee reader should remain owned until reaping")
            .read_to_end(drained)
            .map(|_| ())
    }

    fn close_tee_reader(&mut self) {
        drop(self.tee_reader.take());
    }

    fn join_source_writer(&mut self) -> thread::Result<io::Result<()>> {
        self.source_writer
            .take()
            .expect("source writer should be joined once")
            .join()
    }

    fn join_worker(&mut self) -> thread::Result<()> {
        self.worker
            .take()
            .expect("reader worker should be joined once")
            .join()
    }
}

impl Drop for AttemptRecovery {
    fn drop(&mut self) {
        self.release_barrier();
        self.close_tee_reader();
        if let Some(source_writer) = self.source_writer.take() {
            let _ = source_writer.join();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn anonymous_pipe() -> io::Result<(File, File)> {
    let mut reader: RawHandle = std::ptr::null_mut();
    let mut writer: RawHandle = std::ptr::null_mut();
    // SAFETY: both output pointers are valid; successful handles are
    // immediately transferred to owning `File` values.
    if unsafe { CreatePipe(&mut reader, &mut writer, std::ptr::null(), 4096) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe returned two distinct owned handles.
    Ok(unsafe { (File::from_raw_handle(reader), File::from_raw_handle(writer)) })
}

fn fill_pipe(pipe: &mut File, observer: &File) -> io::Result<u32> {
    let mut capacity = 0;
    // SAFETY: the readable pipe handle is valid and only the outbound
    // capacity is read.
    if unsafe {
        GetNamedPipeInfo(
            observer.as_raw_handle(),
            std::ptr::null_mut(),
            &mut capacity,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if capacity == 0 {
        return Err(io::Error::other("tee pipe reported an empty output buffer"));
    }
    let fill = [0x5a_u8; 1024];
    let mut remaining = capacity;
    while remaining > 0 {
        let count = remaining.min(fill.len() as u32);
        pipe.write_all(&fill[..count as usize])?;
        remaining -= count;
    }
    let mut available = 0;
    // SAFETY: only the queued byte count is requested.
    if unsafe {
        PeekNamedPipe(
            observer.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if available != capacity {
        return Err(io::Error::other(
            "tee pipe did not retain its reported full output buffer",
        ));
    }
    Ok(capacity)
}

const CHILD_ENV: &str = "QUBIT_COMMAND_WINDOWS_PROBE_SCENARIO";
const CHILD_TEST: &str =
    "command_runner::internal::output_collector::tests::windows_probe_tests::probe_child_entry";
struct ChildGuard(Option<std::process::Child>);
impl ChildGuard {
    fn child(&mut self) -> &mut std::process::Child {
        self.0.as_mut().expect("child process should be owned")
    }
    fn reap(&mut self) -> io::Result<std::process::ExitStatus> {
        let status = self.child().wait()?;
        self.0.take();
        Ok(status)
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct ChildOutcome {
    timed_out: bool,
    status: Option<std::process::ExitStatus>,
    log: String,
}
fn supervise_child(scenario: &str, timeout: Duration) -> io::Result<ChildOutcome> {
    let mut log = tempfile::NamedTempFile::new()?;
    let stdout = log.reopen()?;
    let stderr = stdout.try_clone()?;
    let child = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, scenario)
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr))
        .spawn()?;
    let mut guard = ChildGuard(Some(child));
    let started = Instant::now();
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        if let Some(s) = guard.child().try_wait()? {
            break Some(s);
        }
        if Instant::now() >= deadline {
            timed_out = true;
            eprintln!(
                "WINDOWS_PROBE_WATCHDOG_INTERVENTION scenario={scenario} elapsed_ms={}",
                started.elapsed().as_millis()
            );
            guard.child().kill()?;
            break Some(guard.reap()?);
        }
        thread::sleep(Duration::from_millis(10));
    };
    use std::io::Seek;
    log.as_file_mut().rewind()?;
    let mut contents = String::new();
    log.read_to_string(&mut contents)?;
    Ok(ChildOutcome {
        timed_out,
        status,
        log: contents,
    })
}
#[test]
fn probe_child_entry() {
    let Ok(scenario) = std::env::var(CHILD_ENV) else {
        return;
    };
    eprintln!("WINDOWS_PROBE_CHILD_STARTED scenario={scenario}");
    match scenario.as_str() {
        "blocked" => loop {
            thread::park_timeout(Duration::from_secs(60));
        },
        "probe" | "readiness" | "eof-flush" => run_real_probe(scenario.as_str()),
        "stdin-check-write" => run_stdin_probe(),
        "failure" => panic!("intentional child failure"),
        "missing-completion" => return,
        _ => panic!("unknown Windows probe child scenario: {scenario}"),
    }
    eprintln!("WINDOWS_PROBE_CHILD_COMPLETED scenario={scenario}");
}
fn assert_completed(outcome: ChildOutcome, scenario: &str) {
    assert!(
        !outcome.timed_out,
        "child exceeded watchdog; log: {}",
        outcome.log
    );
    assert!(
        outcome.status.expect("child must be waited").success(),
        "child failed; log: {}",
        outcome.log
    );
    assert!(
        outcome
            .log
            .contains(&format!("WINDOWS_PROBE_CHILD_STARTED scenario={scenario}")),
        "missing child start marker: {}",
        outcome.log
    );
    assert!(
        outcome.log.contains(&format!(
            "WINDOWS_PROBE_CHILD_COMPLETED scenario={scenario}"
        )),
        "missing child completion marker: {}",
        outcome.log
    );
    assert!(
        outcome.log.contains("running 1 test"),
        "child test count missing; log: {}",
        outcome.log
    );
    eprintln!("WINDOWS_PROBE_CHILD_LOG scenario={scenario}\n{}", outcome.log);
}
#[test]
fn test_windows_probe_watchdog_reaps_blocked_child() {
    let started = Instant::now();
    let outcome =
        supervise_child("blocked", Duration::from_secs(1)).expect("child supervision should work");
    assert!(
        outcome.timed_out,
        "blocked child must hit watchdog; log: {}",
        outcome.log
    );
    assert!(outcome.status.is_some(), "timed-out child must be waited");
    assert!(
        outcome
            .log
            .contains("WINDOWS_PROBE_CHILD_STARTED scenario=blocked"),
        "blocked child start marker missing: {}",
        outcome.log
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "blocked child was not promptly reaped"
    );
}
#[test]
fn test_windows_real_cancel_read_to_blocked_tee_interleaving() {
    let outcome =
        supervise_child("probe", Duration::from_secs(30)).expect("child supervision should work");
    assert_completed(outcome, "probe");
}

#[test]
fn test_windows_successful_cancel_interleaving_read_to_tee() {
    let outcome =
        supervise_child("probe", Duration::from_secs(30)).expect("child supervision should work");
    assert_completed(outcome, "probe");
}

#[test]
fn test_windows_successful_cancel_interleaving_readiness() {
    let outcome = supervise_child("readiness", Duration::from_secs(30))
        .expect("child supervision should work");
    assert_completed(outcome, "readiness");
}

#[test]
fn test_windows_successful_cancel_interleaving_eof_to_flush() {
    let outcome = supervise_child("eof-flush", Duration::from_secs(30))
        .expect("child supervision should work");
    assert_completed(outcome, "eof-flush");
}

#[test]
fn test_windows_successful_cancel_interleaving_stdin_check_to_write() {
    let outcome = supervise_child("stdin-check-write", Duration::from_secs(30))
        .expect("child supervision should work");
    assert_completed(outcome, "stdin-check-write");
}

#[test]
fn test_windows_probe_controller_rejects_nonzero_child() {
    let outcome =
        supervise_child("failure", Duration::from_secs(5)).expect("child supervision should work");
    assert!(!outcome.timed_out);
    assert!(!outcome.status.expect("child must be waited").success());
    assert!(
        outcome
            .log
            .contains("WINDOWS_PROBE_CHILD_STARTED scenario=failure")
    );
    assert!(std::panic::catch_unwind(|| assert_completed(outcome, "failure")).is_err());
}

#[test]
fn test_windows_probe_controller_rejects_missing_completion_marker() {
    let outcome = supervise_child("missing-completion", Duration::from_secs(5))
        .expect("child supervision should work");
    assert!(!outcome.timed_out);
    assert!(outcome.status.expect("child must be waited").success());
    assert!(
        outcome
            .log
            .contains("WINDOWS_PROBE_CHILD_STARTED scenario=missing-completion")
    );
    assert!(!outcome.log.contains("WINDOWS_PROBE_CHILD_COMPLETED"));
    assert!(outcome.log.contains("running 1 test"));
    assert!(std::panic::catch_unwind(|| assert_completed(outcome, "missing-completion")).is_err());
}

fn run_real_probe(scenario: &str) {
    let mut first_cancel_successes = 0;
    let mut first_cancel_errors = Vec::new();
    let mut tee_write_entries = 0;
    let mut confirmed_blocked_writes = 0;
    let mut source_writer_broken_pipes = 0;
    let mut valid_hits = Vec::new();
    let mut eof_pipe_successful_cancel_flush_hits = Vec::new();

    for attempt in 0..ATTEMPTS {
        let use_readiness = scenario == "readiness";
        let use_eof_flush = scenario == "eof-flush";
        let attempt_start = Instant::now();
        let (source_reader, mut source_writer) =
            anonymous_pipe().expect("source pipe should be created");
        let (tee_reader, mut tee_writer) = anonymous_pipe().expect("tee pipe should be created");
        let tee_capacity = if use_eof_flush {
            0
        } else {
            fill_pipe(&mut tee_writer, &tee_reader).expect("tee pipe should be filled and checked")
        };
        let (event_tx, event_rx) = mpsc::channel();
        let worker_result = Arc::new(Mutex::new(None));
        let worker_result_slot = Arc::clone(&worker_result);
        let (source_release_tx, source_release_rx) = mpsc::channel();
        let (close_source_tx, close_source_rx) = mpsc::channel();
        let (cancellation, token) =
            IoCancellation::pair().expect("Windows cancellation should initialize");
        let token_observer = Arc::clone(&token.cancelled);
        let read_token_observer = Arc::clone(&token.cancelled);
        let barrier = Arc::new(Barrier::new(2));
        let writer_barrier = Arc::clone(&barrier);
        let source_writer_thread = thread::spawn(move || {
            writer_barrier.wait();
            let _ = source_release_tx.send(Instant::now());
            let result = source_writer.write_all(&[0x41]);
            if use_eof_flush {
                let _ = close_source_rx.recv();
            }
            result
        });
        let tee = OutputTee::new(
            Box::new(TeeEntry(tee_writer, event_tx.clone())),
            PathBuf::from("pipe-tee.log"),
        );
        let ready_fd = source_reader.as_raw_handle() as usize;
        let worker = thread::spawn(move || {
            let mut options = OutputCaptureOptions::new(None, Some(tee));
            if use_readiness {
                options = options.with_total_budget(
                    Arc::new(TotalOutputBudget::new(u64::MAX, None, None)),
                    OutputStream::Stdout,
                );
            }
            let result = read_output_inner(
                &mut ReadEntry(
                    source_reader,
                    event_tx.clone(),
                    read_token_observer,
                    use_readiness,
                ),
                options,
                Some(&token),
                if use_readiness {
                    Some(ready_fd as RawHandle)
                } else {
                    None
                },
            );
            *worker_result_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
            let _ = event_tx.send(ProbeEvent::WorkerDone(Instant::now()));
        });
        let mut recovery = AttemptRecovery {
            barrier: Arc::clone(&barrier),
            barrier_released: false,
            source_writer: Some(source_writer_thread),
            worker: Some(worker),
            tee_reader: Some(tee_reader),
        };

        if use_readiness || use_eof_flush {
            // Readiness is checked before the wrapped File::read entry, so
            // make real source data available before waiting for that event.
            recovery.release_barrier();
        }
        let mut completed_before_cancel = false;
        let mut read_entries_seen = 0;
        let read_entry = loop {
            match event_rx
                .recv_timeout(EVENT_TIMEOUT)
                .expect("worker should enter the real source-pipe read")
            {
                event @ ProbeEvent::ReadEntry(at) => {
                    log_event(attempt, attempt_start, event);
                    if use_eof_flush {
                        read_entries_seen += 1;
                        if read_entries_seen < 2 {
                            continue;
                        }
                    }
                    break at;
                }
                event @ ProbeEvent::ReadReturn(_, _, _, _)
                | event @ ProbeEvent::ReadinessReturned(_)
                | event @ ProbeEvent::TeeEntry(_)
                | event @ ProbeEvent::TeeReturn(_, _, _)
                | event @ ProbeEvent::TeeFlushEntry(_)
                | event @ ProbeEvent::TeeFlushReturn(_, _)
                | event @ ProbeEvent::StdinWriteEntry(_)
                | event @ ProbeEvent::StdinWriteReturn(_, _, _, _)
                | event @ ProbeEvent::WorkerDone(_) => {
                    let worker_completed = matches!(&event, ProbeEvent::WorkerDone(_));
                    let readiness_returned = matches!(&event, ProbeEvent::ReadinessReturned(_));
                    log_event(attempt, attempt_start, event);
                    if use_readiness && readiness_returned {
                        continue;
                    }
                    if use_eof_flush && worker_completed {
                        completed_before_cancel = true;
                        break Instant::now();
                    }
                    if use_eof_flush {
                        continue;
                    }
                    panic!("worker entered tee or completed before source read")
                }
            }
        };
        recovery.release_barrier();
        let worker = recovery
            .worker
            .as_ref()
            .expect("worker handle should be live");
        let source_close_thread = if use_eof_flush {
            let close_barrier = Arc::new(Barrier::new(2));
            let close_thread_barrier = Arc::clone(&close_barrier);
            let close_thread = thread::spawn(move || {
                close_thread_barrier.wait();
                let _ = close_source_tx.send(());
            });
            // Release source EOF and invoke cancellation from synchronized
            // peers while the helper is inside its second real blocking read.
            close_barrier.wait();
            Some(close_thread)
        } else {
            None
        };
        let first_cancel_call = Instant::now();
        let first_cancel = cancellation.cancel(worker);
        let first_cancel_return = Instant::now();
        let first_cancel_succeeded = first_cancel.is_ok();
        eprintln!(
            "Windows RC-002 attempt={attempt} cancel_call_ns={} cancel_return_ns={} success={} raw_os_error={:?} shared_cancel_flag_after_api={}",
            first_cancel_call.duration_since(attempt_start).as_nanos(),
            first_cancel_return.duration_since(attempt_start).as_nanos(),
            first_cancel_succeeded,
            first_cancel
                .as_ref()
                .err()
                .and_then(io::Error::raw_os_error),
            token_observer.load(Ordering::Acquire),
        );
        if let Err(source) = first_cancel {
            first_cancel_errors.push(source.raw_os_error());
        } else {
            first_cancel_successes += 1;
        }
        if let Some(close_thread) = source_close_thread {
            close_thread
                .join()
                .expect("source close controller should not panic");
        }
        let source_release = source_release_rx
            .recv_timeout(EVENT_TIMEOUT)
            .expect("source writer should report its barrier release");

        let mut tee_entry = None;
        let mut worker_done = completed_before_cancel;
        let mut worker_events_disconnected = false;
        let mut eof_return = None;
        let mut flush_entry = None;
        while tee_entry.is_none() && !worker_done {
            match event_rx.recv() {
                Ok(
                    event @ (ProbeEvent::ReadEntry(_)
                    | ProbeEvent::ReadinessReturned(_)
                    | ProbeEvent::ReadReturn(_, _, _, _)
                    | ProbeEvent::TeeReturn(_, _, _)
                    | ProbeEvent::TeeFlushEntry(_)
                    | ProbeEvent::TeeFlushReturn(_, _)
                    | ProbeEvent::StdinWriteEntry(_)
                    | ProbeEvent::StdinWriteReturn(_, _, _, _)),
                ) => {
                    match &event {
                        ProbeEvent::ReadReturn(at, Some(0), _, _) => eof_return = Some(*at),
                        ProbeEvent::TeeFlushEntry(at) => flush_entry = Some(*at),
                        _ => {}
                    }
                    log_event(attempt, attempt_start, event);
                }
                Ok(event @ ProbeEvent::TeeEntry(at)) => {
                    log_event(attempt, attempt_start, event);
                    tee_entry = Some(at);
                }
                Ok(event @ ProbeEvent::WorkerDone(_)) => {
                    log_event(attempt, attempt_start, event);
                    worker_done = true;
                }
                Err(_) => {
                    worker_events_disconnected = true;
                    break;
                }
            }
        }
        if use_eof_flush
            && first_cancel_succeeded
            && let (Some(eof_return), Some(flush_entry)) = (eof_return, flush_entry)
            && first_cancel_return < eof_return
            && eof_return < flush_entry
        {
            eof_pipe_successful_cancel_flush_hits.push(attempt);
            eprintln!(
                "Windows RC-002 eof pipe sample attempt={attempt}: successful cancel return precedes EOF return and tee flush entry; pipe substitute only, not ordinary-file defect evidence"
            );
        }
        if tee_entry.is_some() {
            tee_write_entries += 1;
        }
        let mut confirmed_blocked = false;
        if let Some(tee_entry_time) = tee_entry
            && first_cancel_succeeded
            && first_cancel_return < tee_entry_time
        {
            // Observe that the helper remains unfinished first. A
            // successful second cancellation then establishes that the
            // tee WriteFile itself was pending in the kernel.
            thread::sleep(BLOCKED_WRITE_OBSERVATION);
            let remained_unfinished = !worker.is_finished();
            let second_cancel_call = Instant::now();
            let second_cancel = cancel_synchronous_io(worker);
            let second_cancel_return = Instant::now();
            eprintln!(
                "Windows RC-002 attempt={attempt} second_cancel_call_ns={} second_cancel_return_ns={} success={} raw_os_error={:?} worker_unfinished_before_call={remained_unfinished}",
                second_cancel_call.duration_since(attempt_start).as_nanos(),
                second_cancel_return
                    .duration_since(attempt_start)
                    .as_nanos(),
                second_cancel.is_ok(),
                second_cancel
                    .as_ref()
                    .err()
                    .and_then(io::Error::raw_os_error),
            );
            confirmed_blocked = remained_unfinished && second_cancel.is_ok();
        }
        confirmed_blocked_writes += usize::from(confirmed_blocked);
        if let Some(tee_entry_time) = tee_entry
            && first_cancel_succeeded
            && first_cancel_return < tee_entry_time
            && confirmed_blocked
        {
            valid_hits.push((
                attempt,
                attempt_start,
                read_entry,
                source_release,
                first_cancel_call,
                first_cancel_return,
                tee_entry_time,
            ));
        }

        // Close/release every pipe path before unbounded thread joins.
        match recovery
            .join_source_writer()
            .expect("source writer should not panic")
        {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                // Cancellation can close the read handle before this
                // attempt's source writer gets scheduled.
                source_writer_broken_pipes += 1;
            }
            Err(error) => panic!("source writer should release the pipe: {error}"),
        }
        let mut drained = Vec::new();
        recovery
            .drain_tee(&mut drained)
            .expect("tee pipe should drain after its writer is released");
        assert!(drained.len() >= tee_capacity as usize);
        recovery.close_tee_reader();
        if let Err(panic) = recovery.join_worker() {
            std::panic::resume_unwind(panic);
        }
        while let Ok(event) = event_rx.try_recv() {
            log_event(attempt, attempt_start, event);
        }
        if worker_events_disconnected {
            panic!("reader worker event channel disconnected without a completion event");
        }
        let saved_result = worker_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("reader worker should save its result before completion");
        log_worker_result(attempt, saved_result);
        if !valid_hits.is_empty() {
            break;
        }
    }

    let error_not_found_count = first_cancel_errors
        .iter()
        .filter(|error| **error == Some(1168))
        .count();
    eprintln!(
        "Windows RC-002 scenario={scenario} attempts={}, first_cancel_successes={first_cancel_successes}, error_not_found_1168={error_not_found_count}, first_cancel_error_os_codes={first_cancel_errors:?}, tee_write_entries={tee_write_entries}, confirmed_blocked_writes={confirmed_blocked_writes}, source_writer_broken_pipes={source_writer_broken_pipes}, valid_hits={}, eof_pipe_successful_cancel_flush_hits={eof_pipe_successful_cancel_flush_hits:?}",
        valid_hits.last().map_or(ATTEMPTS, |hit| hit.0 + 1),
        valid_hits.len()
    );
    for (
        attempt,
        attempt_start,
        read_entry,
        source_release,
        cancel_call,
        cancel_return,
        tee_entry,
    ) in &valid_hits
    {
        eprintln!(
            "valid hit attempt={attempt} timeline_ns(attempt_start=0, read_entry={}, source_release={}, cancel_call={}, cancel_return={}, tee_write_entry={})",
            read_entry.duration_since(*attempt_start).as_nanos(),
            source_release.duration_since(*attempt_start).as_nanos(),
            cancel_call.duration_since(*attempt_start).as_nanos(),
            cancel_return.duration_since(*attempt_start).as_nanos(),
            tee_entry.duration_since(*attempt_start).as_nanos(),
        );
    }
    assert!(
        valid_hits.is_empty(),
        "confirmed RC-002 schedule reproduced; preserve this trace and repair the production path"
    );
    if valid_hits.is_empty() {
        eprintln!("zero valid hits means this run remains unverified, not that the race is absent");
    }
}

fn log_event(attempt: usize, start: Instant, event: ProbeEvent) {
    let (name, at) = match event {
        ProbeEvent::ReadinessReturned(at) => ("readiness_returned", at),
        ProbeEvent::ReadEntry(at) => ("read_entry", at),
        ProbeEvent::ReadReturn(at, count, raw_error, token_observed) => {
            eprintln!(
                "Windows RC-002 attempt={attempt} read_result_bytes={count:?} read_raw_os_error={raw_error:?} cancel_flag_at_read_return={token_observed}"
            );
            ("read_return", at)
        }
        ProbeEvent::TeeEntry(at) => ("tee_write_entry", at),
        ProbeEvent::TeeReturn(at, written, raw_error) => {
            eprintln!(
                "Windows RC-002 attempt={attempt} tee_write_result_bytes={written:?} tee_write_raw_os_error={raw_error:?}"
            );
            ("tee_write_return", at)
        }
        ProbeEvent::TeeFlushEntry(at) => ("tee_flush_entry", at),
        ProbeEvent::TeeFlushReturn(at, raw_error) => {
            eprintln!("Windows RC-002 attempt={attempt} tee_flush_raw_os_error={raw_error:?}");
            ("tee_flush_return", at)
        }
        ProbeEvent::StdinWriteEntry(at) => ("stdin_write_entry", at),
        ProbeEvent::StdinWriteReturn(at, written, raw_error, token_observed) => {
            eprintln!(
                "Windows RC-002 attempt={attempt} stdin_write_result_bytes={written:?} stdin_write_raw_os_error={raw_error:?} cancel_flag_at_write_return={token_observed}"
            );
            ("stdin_write_return", at)
        }
        ProbeEvent::WorkerDone(at) => ("worker_done", at),
    };
    eprintln!(
        "Windows RC-002 attempt={attempt} event={name} at_ns={}",
        at.duration_since(start).as_nanos()
    );
}

fn log_worker_result(attempt: usize, result: WorkerResult) {
    match result {
        Ok(output) => eprintln!(
            "Windows RC-002 attempt={attempt} worker_result=Ok retained_bytes={} truncated={} complete={}",
            output.bytes.len(),
            output.truncated,
            output.complete,
        ),
        Err(OutputCaptureError::Read { source, output }) => eprintln!(
            "Windows RC-002 attempt={attempt} worker_result=ReadError raw_os_error={:?} retained_bytes={} truncated={} complete={}",
            source.raw_os_error(),
            output.bytes.len(),
            output.truncated,
            output.complete,
        ),
        Err(OutputCaptureError::Write {
            path,
            source,
            output,
        }) => eprintln!(
            "Windows RC-002 attempt={attempt} worker_result=TeeWriteError path={} raw_os_error={:?} retained_bytes={} truncated={} complete={}",
            path.display(),
            source.raw_os_error(),
            output.bytes.len(),
            output.truncated,
            output.complete,
        ),
        Err(OutputCaptureError::ReadAfterWrite {
            path,
            write_source,
            read_source,
            output,
        }) => eprintln!(
            "Windows RC-002 attempt={attempt} worker_result=ReadAfterTeeWriteError path={} tee_raw_os_error={:?} read_raw_os_error={:?} retained_bytes={} truncated={} complete={}",
            path.display(),
            write_source.raw_os_error(),
            read_source.raw_os_error(),
            output.bytes.len(),
            output.truncated,
            output.complete,
        ),
    }
}

fn run_stdin_probe() {
    let mut successful_cancels = 0;
    let mut cancel_errors = Vec::new();
    let mut post_cancel_writes = 0;
    for attempt in 0..ATTEMPTS {
        let attempt_start = Instant::now();
        let (reader, mut writer) = anonymous_pipe().expect("stdin pipe should be created");
        let capacity = fill_pipe(&mut writer, &reader).expect("stdin pipe should be filled");
        let (event_tx, event_rx) = mpsc::channel();
        let (cancellation, token) = IoCancellation::pair().expect("cancellation should initialize");
        let worker_events = event_tx.clone();
        let worker = thread::spawn(move || {
            let result = write_stdin_until_cancelled(
                &mut StdinWriteEntry(writer, worker_events.clone(), Arc::clone(&token.cancelled)),
                &[0x42; 16 * 1024],
                token,
            );
            let _ = worker_events.send(ProbeEvent::WorkerDone(Instant::now()));
            result
        });
        let write_entry = loop {
            match event_rx
                .recv_timeout(EVENT_TIMEOUT)
                .expect("stdin write should enter")
            {
                event @ ProbeEvent::StdinWriteEntry(at) => {
                    log_event(attempt, attempt_start, event);
                    break at;
                }
                event => log_event(attempt, attempt_start, event),
            }
        };
        let cancel_call = Instant::now();
        let cancel_result = cancellation.cancel(&worker);
        let cancel_return = Instant::now();
        eprintln!(
            "Windows RC-002 scenario=stdin-check-to-write attempt={attempt} pipe_capacity={capacity} write_entry_ns={} cancel_call_ns={} cancel_return_ns={} success={} raw_os_error={:?}",
            write_entry.duration_since(attempt_start).as_nanos(),
            cancel_call.duration_since(attempt_start).as_nanos(),
            cancel_return.duration_since(attempt_start).as_nanos(),
            cancel_result.is_ok(),
            cancel_result
                .as_ref()
                .err()
                .and_then(io::Error::raw_os_error),
        );
        if cancel_result.is_ok() {
            successful_cancels += 1;
        } else {
            cancel_errors.push(
                cancel_result
                    .as_ref()
                    .err()
                    .and_then(io::Error::raw_os_error),
            );
            // Release a still-blocked pipe write if cancellation did not
            // interrupt it; the child watchdog remains the final outer bound.
            drop(reader);
        }
        let mut write_returns = 0;
        let mut later_writes = 0;
        let mut done = false;
        while !done {
            match event_rx
                .recv_timeout(EVENT_TIMEOUT)
                .expect("stdin worker should finish")
            {
                event @ ProbeEvent::StdinWriteEntry(_) => {
                    later_writes += 1;
                    log_event(attempt, attempt_start, event);
                }
                event @ ProbeEvent::StdinWriteReturn(_, _, _, _) => {
                    write_returns += 1;
                    log_event(attempt, attempt_start, event);
                }
                event @ ProbeEvent::WorkerDone(_) => {
                    log_event(attempt, attempt_start, event);
                    done = true;
                }
                event => log_event(attempt, attempt_start, event),
            }
        }
        let result = worker.join().expect("stdin helper should not panic");
        post_cancel_writes += later_writes;
        eprintln!(
            "Windows RC-002 scenario=stdin-check-to-write attempt={attempt} write_returns={write_returns} post_cancel_write_entries={later_writes} worker_result={result:?} cancel_success={}",
            cancel_result.is_ok()
        );
        if cancel_result.is_ok() && later_writes > 0 {
            panic!("successful cancellation was followed by a later stdin write entry");
        }
        if cancel_result.is_ok() {
            break;
        }
    }
    eprintln!(
        "Windows RC-002 stdin coverage attempts_max={ATTEMPTS} successful_cancels={successful_cancels} cancel_error_os_codes={cancel_errors:?} post_cancel_write_entries={post_cancel_writes}"
    );
    if successful_cancels == 0 {
        eprintln!("zero successful stdin cancellations is inconclusive, not safety proof");
    }
}
