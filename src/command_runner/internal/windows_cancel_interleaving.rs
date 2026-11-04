// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows-only real-pipe cancellation interleaving probe.

#![cfg(all(test, windows))]

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::RawHandle;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use super::cancel::cancel_synchronous_io;
use super::captured_output::CapturedOutput;
use super::io_cancellation::IoCancellation;
use super::output_capture_error::OutputCaptureError;
use super::output_capture_options::OutputCaptureOptions;
use super::output_collector::read_output_inner;
use super::output_tee::OutputTee;

const ATTEMPTS: usize = 128;
const CHILD_MODE: &str = "QUBIT_WINDOWS_CANCEL_INTERLEAVING_CHILD";
const TEST_FILTER: &str =
    "command_runner::internal::windows_cancel_interleaving::windows_successful_cancel_interleaving";
const WATCHDOG_LIMIT: Duration = Duration::from_secs(30);
const EVENT_TIMEOUT: Duration = Duration::from_secs(2);
const BLOCKED_WRITE_OBSERVATION: Duration = Duration::from_millis(10);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreatePipe(read_pipe: *mut RawHandle, write_pipe: *mut RawHandle, attributes: *const c_void, size: u32) -> i32;
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

struct ReadEntry(File, mpsc::Sender<Instant>);

impl Read for ReadEntry {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let _ = self.1.send(Instant::now());
        self.0.read(buffer)
    }
}

struct TeeEntry(File, mpsc::Sender<Instant>);

impl Write for TeeEntry {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let _ = self.1.send(Instant::now());
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
    worker: Option<thread::JoinHandle<WorkerResult>>,
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

    fn join_worker(&mut self) -> thread::Result<WorkerResult> {
        self.worker.take().expect("reader worker should be joined once").join()
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

#[test]
fn windows_successful_cancel_interleaving() {
    match std::env::var(CHILD_MODE).as_deref() {
        Ok("probe") => run_read_to_tee_probe(),
        Ok("watchdog_fault") => run_watchdog_fault_self_test(),
        _ => {
            let fault = run_child("watchdog_fault", Duration::from_secs(1));
            assert!(
                !fault.timed_out,
                "watchdog self-test should release the blocked helper before the process deadline"
            );
            assert!(fault.reaped, "watchdog self-test child should be reaped");
            assert_eq!(
                fault.status.and_then(|status| status.code()),
                Some(0),
                "real-pipe watchdog self-test failed"
            );
            eprintln!(
                "{}",
                r#"{"event":"watchdog_parent_check","child_reaped":true,"child_kill_used":false}"#
            );
            let probe = run_child("probe", WATCHDOG_LIMIT);
            assert!(!probe.timed_out, "Windows real-pipe probe exceeded watchdog limit");
            assert_eq!(
                probe.status.and_then(|status| status.code()),
                Some(0),
                "probe child failed"
            );
        }
    }
}

struct ChildOutcome {
    timed_out: bool,
    reaped: bool,
    status: Option<ExitStatus>,
}

fn run_watchdog_fault_self_test() {
    let (reader, mut writer) = anonymous_pipe().expect("watchdog self-test pipe should be created");
    let capacity = fill_pipe(&mut writer, &reader).expect("watchdog self-test pipe should be filled");
    let helper = thread::spawn(move || writer.write_all(b"x"));
    thread::sleep(Duration::from_millis(50));
    assert!(!helper.is_finished(), "helper should block while the real pipe is full");
    let watchdog = thread::spawn(move || {
        let mut reader = reader;
        let mut drained = Vec::new();
        reader
            .read_to_end(&mut drained)
            .expect("watchdog should drain the blocked pipe");
        (drained.len(), capacity)
    });
    helper
        .join()
        .expect("blocked pipe helper should be reaped")
        .expect("released helper write should succeed");
    let (drained, capacity) = watchdog.join().expect("pipe watchdog should be reaped");
    assert!(drained >= capacity as usize, "watchdog should drain the filled pipe");
    eprintln!(
        "{}",
        r#"{"event":"watchdog_self_test","blocked_real_pipe":true,"watchdog_released_pipe":true,"helper_reaped":true,"child_kill_used":false}"#
    );
}

fn run_child(mode: &str, timeout: Duration) -> ChildOutcome {
    let executable = std::env::current_exe().expect("test harness executable should be available");
    let mut child = Command::new(executable)
        .args(["--exact", TEST_FILTER, "--nocapture"])
        .env(CHILD_MODE, mode)
        .spawn()
        .expect("watchdog child should start");
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("watchdog should inspect child status") {
            Some(status) => {
                return ChildOutcome {
                    timed_out: false,
                    reaped: true,
                    status: Some(status),
                };
            }
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            None => {
                child.kill().expect("watchdog should terminate overdue child");
                let status = child.wait().expect("watchdog should reap terminated child");
                return ChildOutcome {
                    timed_out: true,
                    reaped: true,
                    status: Some(status),
                };
            }
        }
    }
}

fn run_read_to_tee_probe() {
    let mut first_cancel_successes = 0;
    let mut first_cancel_errors = Vec::new();
    let mut tee_write_entries = 0;
    let mut confirmed_blocked_writes = 0;
    let mut source_writer_broken_pipes = 0;
    let mut legacy_candidates = Vec::new();

    for attempt in 0..ATTEMPTS {
        let attempt_start = Instant::now();
        let (source_reader, mut source_writer) = anonymous_pipe().expect("source pipe should be created");
        let (tee_reader, mut tee_writer) = anonymous_pipe().expect("tee pipe should be created");
        let tee_capacity = fill_pipe(&mut tee_writer, &tee_reader).expect("tee pipe should be filled and checked");
        let (read_tx, read_rx) = mpsc::channel();
        let (tee_tx, tee_rx) = mpsc::channel();
        let (source_release_tx, source_release_rx) = mpsc::channel();
        let (cancellation, token) = IoCancellation::pair().expect("Windows cancellation should initialize");
        let barrier = Arc::new(Barrier::new(2));
        let writer_barrier = Arc::clone(&barrier);
        let source_writer_thread = thread::spawn(move || {
            writer_barrier.wait();
            let _ = source_release_tx.send(Instant::now());
            source_writer.write_all(&[0x41])
        });
        let tee = OutputTee::new(Box::new(TeeEntry(tee_writer, tee_tx)), PathBuf::from("pipe-tee.log"));
        let worker = thread::spawn(move || {
            read_output_inner(
                &mut ReadEntry(source_reader, read_tx),
                OutputCaptureOptions::new(None, Some(tee)),
                Some(&token),
                None,
            )
        });
        let mut recovery = AttemptRecovery {
            barrier: Arc::clone(&barrier),
            barrier_released: false,
            source_writer: Some(source_writer_thread),
            worker: Some(worker),
            tee_reader: Some(tee_reader),
        };

        let read_entry = read_rx
            .recv_timeout(EVENT_TIMEOUT)
            .expect("worker should enter the real source-pipe read");
        recovery.release_barrier();
        let worker = recovery.worker.as_ref().expect("worker handle should be live");
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

        let tee_entry = tee_rx.recv_timeout(EVENT_TIMEOUT).ok();
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
            legacy_candidates.push((
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
        match recovery.join_source_writer().expect("source writer should not panic") {
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
        let _ = recovery.join_worker().expect("reader helper should be recoverable");
        if !legacy_candidates.is_empty() {
            break;
        }
    }

    let error_not_found_count = first_cancel_errors.iter().filter(|error| **error == Some(1168)).count();
    eprintln!(
        "Windows RC-002 read-to-tee attempts={}, first_cancel_successes={first_cancel_successes}, error_not_found_1168={error_not_found_count}, first_cancel_error_os_codes={first_cancel_errors:?}, tee_write_entries={tee_write_entries}, legacy_confirmed_blocked_writes={confirmed_blocked_writes}, source_writer_broken_pipes={source_writer_broken_pipes}, legacy_candidate_count={}",
        legacy_candidates.last().map_or(ATTEMPTS, |hit| hit.0 + 1),
        legacy_candidates.len()
    );
    for (attempt, attempt_start, read_entry, source_release, cancel_call, cancel_return, tee_entry) in
        &legacy_candidates
    {
        eprintln!(
            "legacy candidate attempt={attempt} timeline_ns(attempt_start=0, read_entry={}, source_release={}, cancel_call={}, cancel_return={}, tee_write_entry={})",
            read_entry.duration_since(*attempt_start).as_nanos(),
            source_release.duration_since(*attempt_start).as_nanos(),
            cancel_call.duration_since(*attempt_start).as_nanos(),
            cancel_return.duration_since(*attempt_start).as_nanos(),
            tee_entry.duration_since(*attempt_start).as_nanos(),
        );
    }
    if legacy_candidates.is_empty() {
        eprintln!("zero partial-probe hits is not a finding about the production cleanup path");
    }
    eprintln!(
        r#"{{"event":"scenario_classification","schema":1,"scenario":"read_to_tee","classification":"uncovered","attempts":{},"partial_probe_hits":{},"reason":"probe calls read_output_inner directly and reaps by a second cancellation plus manual pipe drain; it does not establish that production OutputReader cleanup join requires watchdog rescue"}}"#,
        legacy_candidates.last().map_or(ATTEMPTS, |hit| hit.0 + 1),
        legacy_candidates.len(),
    );
    eprintln!(
        "{}",
        r#"{"event":"scenario_classification","schema":1,"scenario":"eof_to_flush","classification":"uncovered","attempts":0,"reason":"production tee is an unbuffered File writer; no buffered bytes remain for a blocking flush after EOF. A BufferWriter/BufWriter would change the production path and cannot establish this interleaving"}"#
    );
    eprintln!(
        "{}",
        r#"{"event":"scenario_classification","schema":1,"scenario":"stdin_check_to_write","classification":"uncovered","attempts":0,"reason":"Windows wait_writable only checks the cancellation token; there is no pending synchronous I/O in the check-to-write gap for CancelSynchronousIo to successfully cancel. A success would target the write after it entered the kernel"}"#
    );
    eprintln!(
        "{}",
        r#"{"event":"overall_classification","schema":1,"rc002":"unresolved","reason":"all three planned scenarios lack a complete witness satisfying successful cancellation, normal completion, subsequent blocked I/O, production cleanup join, and watchdog release"}"#
    );
    assert!(
        legacy_candidates.is_empty(),
        "legacy read-to-tee candidate observed; preserve this trace, but do not classify it without a watchdog-rescued production cleanup join"
    );
}
