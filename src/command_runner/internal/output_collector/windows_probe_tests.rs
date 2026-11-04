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
use super::super::read_output_inner;

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
    ReadEntry(Instant),
    TeeEntry(Instant),
    WorkerDone(Instant),
}

struct ReadEntry(File, mpsc::Sender<ProbeEvent>);

impl Read for ReadEntry {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let _ = self.1.send(ProbeEvent::ReadEntry(Instant::now()));
        self.0.read(buffer)
    }
}

struct TeeEntry(File, mpsc::Sender<ProbeEvent>);

impl Write for TeeEntry {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let _ = self.1.send(ProbeEvent::TeeEntry(Instant::now()));
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
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
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        if let Some(s) = guard.child().try_wait()? {
            break Some(s);
        }
        if Instant::now() >= deadline {
            timed_out = true;
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
        "probe" => run_real_probe(),
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

fn run_real_probe() {
    let mut first_cancel_successes = 0;
    let mut first_cancel_errors = Vec::new();
    let mut tee_write_entries = 0;
    let mut confirmed_blocked_writes = 0;
    let mut source_writer_broken_pipes = 0;
    let mut valid_hits = Vec::new();

    for attempt in 0..ATTEMPTS {
        let attempt_start = Instant::now();
        let (source_reader, mut source_writer) =
            anonymous_pipe().expect("source pipe should be created");
        let (tee_reader, mut tee_writer) = anonymous_pipe().expect("tee pipe should be created");
        let tee_capacity =
            fill_pipe(&mut tee_writer, &tee_reader).expect("tee pipe should be filled and checked");
        let (event_tx, event_rx) = mpsc::channel();
        let worker_result = Arc::new(Mutex::new(None));
        let worker_result_slot = Arc::clone(&worker_result);
        let (source_release_tx, source_release_rx) = mpsc::channel();
        let (cancellation, token) =
            IoCancellation::pair().expect("Windows cancellation should initialize");
        let barrier = Arc::new(Barrier::new(2));
        let writer_barrier = Arc::clone(&barrier);
        let source_writer_thread = thread::spawn(move || {
            writer_barrier.wait();
            let _ = source_release_tx.send(Instant::now());
            source_writer.write_all(&[0x41])
        });
        let tee = OutputTee::new(
            Box::new(TeeEntry(tee_writer, event_tx.clone())),
            PathBuf::from("pipe-tee.log"),
        );
        let worker = thread::spawn(move || {
            let result = read_output_inner(
                &mut ReadEntry(source_reader, event_tx.clone()),
                OutputCaptureOptions::new(None, Some(tee)),
                Some(&token),
                None,
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

        let read_entry = loop {
            match event_rx
                .recv_timeout(EVENT_TIMEOUT)
                .expect("worker should enter the real source-pipe read")
            {
                ProbeEvent::ReadEntry(at) => break at,
                ProbeEvent::TeeEntry(_) | ProbeEvent::WorkerDone(_) => {
                    panic!("worker entered tee or completed before source read")
                }
            }
        };
        recovery.release_barrier();
        let worker = recovery
            .worker
            .as_ref()
            .expect("worker handle should be live");
        let first_cancel_call = Instant::now();
        let first_cancel = cancellation.cancel(worker);
        let first_cancel_return = Instant::now();
        let first_cancel_succeeded = first_cancel.is_ok();
        if let Err(source) = first_cancel {
            first_cancel_errors.push(source.raw_os_error());
        } else {
            first_cancel_successes += 1;
        }
        let source_release = source_release_rx
            .recv_timeout(EVENT_TIMEOUT)
            .expect("source writer should report its barrier release");

        let mut tee_entry = None;
        let mut worker_done = false;
        let mut worker_events_disconnected = false;
        while tee_entry.is_none() && !worker_done {
            match event_rx.recv() {
                Ok(ProbeEvent::ReadEntry(_)) => {}
                Ok(ProbeEvent::TeeEntry(at)) => tee_entry = Some(at),
                Ok(ProbeEvent::WorkerDone(_)) => worker_done = true,
                Err(_) => {
                    worker_events_disconnected = true;
                    break;
                }
            }
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
            confirmed_blocked = remained_unfinished && cancel_synchronous_io(worker).is_ok();
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
        if worker_events_disconnected {
            panic!("reader worker event channel disconnected without a completion event");
        }
        let _saved_result = worker_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("reader worker should save its result before completion");
        if !valid_hits.is_empty() {
            break;
        }
    }

    let error_not_found_count = first_cancel_errors
        .iter()
        .filter(|error| **error == Some(1168))
        .count();
    eprintln!(
        "Windows RC-002 read-to-tee attempts={}, first_cancel_successes={first_cancel_successes}, error_not_found_1168={error_not_found_count}, first_cancel_error_os_codes={first_cancel_errors:?}, tee_write_entries={tee_write_entries}, confirmed_blocked_writes={confirmed_blocked_writes}, source_writer_broken_pipes={source_writer_broken_pipes}, valid_hits={}",
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
