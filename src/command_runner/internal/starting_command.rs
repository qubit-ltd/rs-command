// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owns a child process while its I/O helpers are being started.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use process_wrap::std::ChildWrapper;

use super::command_io::CommandIo;
use super::command_io::cancel_and_join_started_helpers;
use super::finished_starting_command::FinishedStartingCommand;
use super::managed_child_process::ManagedChildProcess;
use super::output_reader::OutputReader;
use super::process_terminator::ProcessTerminator;
use super::stdin_writer::OptionalStdinWriter;
use crate::CommandCleanupFailure;
use crate::CommandError;
use crate::execution::backend::Reaper;
use crate::execution::backend::ReaperObserver;
use crate::execution::backend::ReaperOwner;
use crate::execution::contracts::ReservationLease;
use crate::session::contracts::SessionBridge;

/// Guards a spawned child until all runner-side I/O helpers are ready.
///
/// Dropping an unfinished guard performs best-effort process termination and
/// joins any helpers that were started.
#[must_use = "dropping an unfinished command guard terminates the child"]
pub(in crate::command_runner) struct StartingCommand<'a> {
    /// Redacted command text used in cleanup logs.
    command: &'a str,
    /// Spawned child process, moved out only after successful initialization.
    child_process: Option<ManagedChildProcess>,
    /// Started stdout reader, if initialization reached that stage.
    stdout_reader: Option<OutputReader>,
    /// Started stderr reader, if initialization reached that stage.
    stderr_reader: Option<OutputReader>,
    /// Optional started stdin writer.
    stdin_writer: OptionalStdinWriter,
    /// Additional managed capacity retained during startup cleanup.
    capacity_lease: Option<ReservationLease>,
    /// Recovery observer for ownership handed to the reaper.
    reaper_observer: Option<ReaperObserver>,
    /// Session bridge closed while startup helpers are being reclaimed.
    session_bridge: Option<Arc<dyn SessionBridge>>,
}

impl<'a> StartingCommand<'a> {
    /// Takes ownership of a newly spawned child process.
    ///
    /// # Parameters
    ///
    /// * `command` - Redacted command text used in cleanup logs.
    /// * `child_process` - Newly spawned child process.
    ///
    /// # Returns
    ///
    /// A guard that terminates the child unless initialization finishes.
    #[inline]
    #[cfg(test)]
    pub(in crate::command_runner) const fn new(
        command: &'a str,
        child_process: ManagedChildProcess,
    ) -> Self {
        Self {
            command,
            child_process: Some(child_process),
            stdout_reader: None,
            stderr_reader: None,
            stdin_writer: None,
            capacity_lease: None,
            reaper_observer: None,
            session_bridge: None,
        }
    }

    /// Creates a startup guard carrying managed cleanup resources.
    ///
    /// # Parameters
    ///
    /// * `command` - Redacted command label for cleanup diagnostics.
    /// * `child_process` - Spawned child whose ownership is guarded.
    /// * `capacity_lease` - Reservation held until cleanup or reaping completes.
    /// * `reaper_observer` - Observer for uncertain child ownership.
    /// * `session_bridge` - Optional session to stop when startup aborts.
    ///
    /// # Returns
    ///
    /// A guard that owns the child and managed cleanup resources.
    pub(in crate::command_runner) fn new_managed(
        command: &'a str,
        child_process: ManagedChildProcess,
        capacity_lease: Option<ReservationLease>,
        reaper_observer: Option<ReaperObserver>,
        session_bridge: Option<Arc<dyn SessionBridge>>,
    ) -> Self {
        Self {
            command,
            child_process: Some(child_process),
            stdout_reader: None,
            stderr_reader: None,
            stdin_writer: None,
            capacity_lease,
            reaper_observer,
            session_bridge,
        }
    }

    /// Returns the guarded child process mutably.
    ///
    /// # Returns
    ///
    /// Child wrapper used to take configured standard-I/O pipes.
    ///
    /// # Panics
    ///
    /// Panics after ownership has transferred to a running command.
    #[must_use]
    #[inline]
    pub(in crate::command_runner) fn child_process(&mut self) -> &mut dyn ChildWrapper {
        self.child_process
            .as_mut()
            .expect("a starting command always owns its child")
            .wrapper_mut()
    }

    /// Records the optional stdin writer started for this child.
    ///
    /// # Parameters
    ///
    /// * `writer` - Optional stdin writer helper.
    #[inline]
    pub(in crate::command_runner) fn set_stdin_writer(&mut self, writer: OptionalStdinWriter) {
        self.stdin_writer = writer;
    }

    /// Records the stdout reader started for this child.
    ///
    /// # Parameters
    ///
    /// * `reader` - Stdout reader helper.
    #[inline]
    pub(in crate::command_runner) fn set_stdout_reader(&mut self, reader: OutputReader) {
        self.stdout_reader = Some(reader);
    }

    /// Records the stderr reader started for this child.
    ///
    /// # Parameters
    ///
    /// * `reader` - Stderr reader helper.
    #[inline]
    pub(in crate::command_runner) fn set_stderr_reader(&mut self, reader: OutputReader) {
        self.stderr_reader = Some(reader);
    }

    /// Transfers a fully initialized child and its helpers to running state.
    ///
    /// # Returns
    ///
    /// The guarded child and complete I/O helper bundle.
    ///
    /// # Panics
    ///
    /// Panics if the child process or either output reader has not been
    /// registered.
    #[must_use = "transfer both the child and its I/O helpers to running state"]
    #[inline]
    pub(in crate::command_runner) fn finish(mut self) -> FinishedStartingCommand {
        let child_process = self
            .child_process
            .take()
            .expect("a starting command always owns its child");
        let stdout_reader = self
            .stdout_reader
            .take()
            .expect("stdout reader must be started before finishing");
        let stderr_reader = self
            .stderr_reader
            .take()
            .expect("stderr reader must be started before finishing");
        let stdin_writer = self.stdin_writer.take();
        let capacity_lease = self.capacity_lease.take();
        let reaper_observer = self.reaper_observer.take();
        let session_bridge = self.session_bridge.take();
        FinishedStartingCommand {
            child_process,
            io: CommandIo::new(stdout_reader, stderr_reader, stdin_writer),
            capacity_lease,
            reaper_observer,
            session_bridge,
        }
    }

    /// Explicitly cleans up a failed initialization and preserves its primary
    /// error.
    ///
    /// Consumes this startup state and cancels all started helpers even when
    /// process termination fails. Any additional failure is attached to the
    /// returned error instead of logged.
    ///
    /// # Parameters
    ///
    /// * `primary` - The startup error to preserve as the primary failure.
    ///
    /// # Returns
    ///
    /// The primary error, retaining its reason and classification, with any
    /// process-termination or helper-cleanup failures attached. Cleanup is
    /// attempted but is not guaranteed to stop every process or helper.
    pub(in crate::command_runner) fn abort(mut self, primary: CommandError) -> CommandError {
        primary.with_cleanup_failures(self.cleanup())
    }

    /// Transfers a partially initialized managed process and its helpers to a
    /// reaper worker.
    pub(in crate::command_runner) fn abort_managed(
        mut self,
        primary: CommandError,
    ) -> CommandError {
        let Some(observer) = self.reaper_observer.take() else {
            return self.abort(primary);
        };
        observer.mark_pending();
        if let Some(bridge) = &self.session_bridge {
            bridge.request_output_stop();
            bridge.close_stdin();
        }
        let command = self.command.to_owned();
        let Some(mut child) = self.child_process.take() else {
            return primary;
        };
        let mut stdout_reader = self.stdout_reader.take();
        let mut stderr_reader = self.stderr_reader.take();
        let mut stdin_writer = self.stdin_writer.take();
        let capacity_lease = self.capacity_lease.take();
        let cleanup_observer = observer.clone();
        let cleanup_bridge = self.session_bridge.take();
        let owner = ReaperOwner::new(move || {
            let mut terminated = false;
            loop {
                if let Some(bridge) = &cleanup_bridge {
                    bridge.request_output_stop();
                    bridge.close_stdin();
                }
                if let Some(reader) = stdout_reader.as_ref() {
                    let _ = reader.cancel();
                }
                if let Some(reader) = stderr_reader.as_ref() {
                    let _ = reader.cancel();
                }
                if let Some(writer) = stdin_writer.as_ref() {
                    let _ = writer.cancel();
                }
                if !terminated {
                    match ProcessTerminator::new(&mut child).terminate(None) {
                        Ok(_) => terminated = true,
                        Err(error) => {
                            log::error!("startup process cleanup remains uncertain: {error:?}");
                        }
                    }
                }
                let helpers_finished = stdout_reader
                    .as_ref()
                    .is_none_or(|helper| helper.is_finished())
                    && stderr_reader
                        .as_ref()
                        .is_none_or(|helper| helper.is_finished())
                    && stdin_writer
                        .as_ref()
                        .is_none_or(|helper| helper.is_finished());
                if terminated && helpers_finished {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let failures = cancel_and_join_started_helpers(
                &command,
                stdout_reader.take(),
                stderr_reader.take(),
                stdin_writer.take(),
            );
            if !failures.is_empty() {
                log::error!("startup helper cleanup failures: {failures:?}");
            }
            drop(capacity_lease);
            cleanup_observer.complete();
        });
        match Reaper::global() {
            Ok(reaper) => match reaper.handoff(owner) {
                Ok(()) => {}
                Err(owner) => observer.retain_or_defer_reaper_owner(owner),
            },
            Err(error) => {
                log::error!("failed to start process reaper: {error}");
                observer.retain_or_defer_reaper_owner(owner);
            }
        }
        primary
    }

    /// Takes all resources, terminates the child and joins the started helpers.
    ///
    /// Returns every observed cleanup failure. Repeated calls are harmless;
    /// failed kill requests with unknown status never enter a blocking wait.
    fn cleanup(&mut self) -> Vec<CommandCleanupFailure> {
        let mut failures = Vec::new();
        if let Some(mut child) = self.child_process.take() {
            match ProcessTerminator::new(&mut child).terminate(None) {
                Ok(outcome) => failures.extend(outcome.cleanup_failures),
                Err(error) => failures.extend(error.into_cleanup_failures()),
            }
        }
        failures.extend(cancel_and_join_started_helpers(
            self.command,
            self.stdout_reader.take(),
            self.stderr_reader.take(),
            self.stdin_writer.take(),
        ));
        failures
    }
}

impl Drop for StartingCommand<'_> {
    /// Reuses explicit cleanup as a fallback during unwinding.
    fn drop(&mut self) {
        for failure in self.cleanup() {
            log::error!(
                "Command '{}' failed during startup cleanup: {failure:?}",
                self.command
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::process::Command as ProcessCommand;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    use qubit_clock::TimeError;

    use super::super::managed_child_process::ManagedChildProcess;
    use super::super::scripted_child::ScriptedChild;
    use super::super::scripted_child::raw_child;
    use super::StartingCommand;
    use crate::CommandCleanupFailure;
    use crate::CommandError;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;
    use crate::OutputStream;

    #[test]
    fn test_managed_startup_abort_retains_child_helpers_and_capacity_until_reaper_cleanup() {
        use std::thread;
        use std::time::Duration;

        use super::super::captured_output::CapturedOutput;
        use super::super::io_cancellation::IoCancellation;
        use super::super::output_reader::OutputReader;
        use crate::execution::backend::ReaperObserver;
        use crate::execution::contracts::ReservationLease;

        let process = ProcessCommand::new("sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .expect("test process should start");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let direct = super::super::scripted_child::ScriptedChild::new(
            "child",
            Box::new(process),
            Arc::clone(&calls),
        )
        .kill_error(io::Error::other("first kill attempt fails"))
        .delegate_kill_to_inner();
        let child = super::super::scripted_child::ScriptedChild::new(
            "tree",
            Box::new(direct),
            Arc::clone(&calls),
        )
        .kill_error(io::Error::other("first tree kill attempt fails"))
        .delegate_kill_to_inner();
        let (observer, completed) = ReaperObserver::pair();
        let released = Arc::new(AtomicBool::new(false));
        let lease_released = Arc::clone(&released);
        let lease = ReservationLease::new(move || lease_released.store(true, Ordering::SeqCst));
        let (unblock_tx, unblock_rx) = mpsc::channel();
        let (cancel, _token) = IoCancellation::pair().expect("cancellation should initialize");
        let reader = OutputReader::new(
            thread::spawn(move || {
                let _ = unblock_rx.recv();
                Ok(CapturedOutput::default())
            }),
            cancel,
        );
        let mut guard = StartingCommand::new_managed(
            "redacted",
            ManagedChildProcess::new(Box::new(child), true),
            Some(lease),
            Some(observer.clone()),
            None,
        );
        guard.set_stdout_reader(reader);
        let error = guard.abort_managed(CommandError::from_reason(
            "redacted",
            CommandErrorReason::TimeFailed {
                source: TimeError::InstantOverflow,
            },
            None,
        ));

        assert_eq!(error.kind(), CommandErrorKind::TimeFailed);
        assert!(observer.is_pending());
        thread::sleep(Duration::from_millis(30));
        assert!(!released.load(Ordering::SeqCst));

        unblock_tx.send(()).expect("helper should be released");
        completed
            .recv_timeout(Duration::from_secs(3))
            .expect("reaper should finish child and helper cleanup");
        assert!(released.load(Ordering::SeqCst));
        assert!(
            calls
                .lock()
                .expect("calls should be readable")
                .iter()
                .filter(|call| call.as_str() == "child.kill")
                .count()
                >= 2
        );
    }

    #[cfg(feature = "async")]
    #[test]
    fn test_managed_startup_abort_wakes_full_session_output_writer() {
        use std::time::Duration;

        use super::super::captured_output::CapturedOutput;
        use super::super::io_cancellation::IoCancellation;
        use super::super::output_reader::OutputReader;
        use crate::execution::backend::ReaperObserver;
        use crate::session::channel::SessionChannels;
        use crate::session::contracts::SessionBridge;

        let channels = SessionChannels::new();
        let bridge: Arc<dyn SessionBridge> = channels.bridge.clone();
        let chunk = vec![0x5a; 16 * 1024];
        for _ in 0..4 {
            bridge
                .write_stdout(&chunk)
                .expect("buffer should accept one complete channel capacity");
        }
        let writer_bridge = Arc::clone(&bridge);
        let reader = OutputReader::new(
            std::thread::spawn(move || {
                writer_bridge
                    .write_stdout(b"blocked startup output")
                    .expect("managed startup abort should wake the blocked writer");
                Ok(CapturedOutput::default())
            }),
            IoCancellation::pair()
                .expect("reader cancellation should initialize")
                .0,
        );
        channels.bridge.wait_for_stdout_writer_for_test();

        let (observer, completed) = ReaperObserver::pair();
        let mut guard = StartingCommand::new_managed(
            "redacted",
            ManagedChildProcess::new(raw_child(), false),
            None,
            Some(observer),
            Some(bridge.clone()),
        );
        guard.set_stdout_reader(reader);
        let _error = guard.abort_managed(CommandError::from_reason(
            "redacted",
            CommandErrorReason::TimeFailed {
                source: TimeError::InstantOverflow,
            },
            None,
        ));

        let completed_without_endpoint_drop =
            completed.recv_timeout(Duration::from_millis(150)).is_ok();
        if !completed_without_endpoint_drop {
            // Release the owner even after a regression so the test never strands a reaper
            // worker.
            bridge.request_output_stop();
            completed
                .recv_timeout(Duration::from_secs(2))
                .expect("test cleanup should release the blocked writer");
        }
        assert!(
            completed_without_endpoint_drop,
            "startup abort must stop session output before waiting for helpers"
        );
        assert!(bridge.stdout_incomplete());
    }

    #[test]
    fn test_startup_abort_retains_primary_without_waiting_after_failed_kills() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let direct = ScriptedChild::new("child", raw_child(), Arc::clone(&calls))
            .kill_error(io::Error::other("child denied"));
        let tree = ScriptedChild::new("tree", Box::new(direct), Arc::clone(&calls))
            .kill_error(io::Error::other("tree denied"))
            .try_wait_results((0..9).map(|_| Ok(None)));
        let guard =
            StartingCommand::new("redacted", ManagedChildProcess::new(Box::new(tree), true));
        let primary = CommandError::from_reason(
            "redacted",
            CommandErrorReason::StartOutputThreadFailed {
                stream: OutputStream::Stderr,
                source: io::Error::other("thread unavailable"),
            },
            None,
        );
        let error = guard.abort(primary);
        assert_eq!(error.kind(), CommandErrorKind::StartOutputThreadFailed);
        assert!(matches!(
            error.cleanup_failures(),
            [
                CommandCleanupFailure::ProcessTreeTermination { .. },
                CommandCleanupFailure::ChildTermination { .. }
            ]
        ));
        let calls = calls.lock().expect("calls should be readable");
        assert!(!calls.iter().any(|call| call.ends_with(".wait")));
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.as_str() == "child.kill")
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.as_str() == "tree.kill")
                .count(),
            1
        );
    }

    #[test]
    fn test_startup_abort_cancels_every_initialized_helper_once() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        use std::thread;

        use super::super::captured_output::CapturedOutput;
        use super::super::io_cancellation::IoCancellation;
        use super::super::output_capture_error::OutputCaptureError;
        use super::super::output_reader::OutputReader;
        use super::super::stdin_writer::StdinWriter;
        for (with_stdin, readers) in [(false, 0), (true, 0), (true, 1), (false, 2), (true, 2)] {
            let stage = usize::from(with_stdin) + readers;
            let calls = Arc::new(Mutex::new(Vec::new()));
            let tree = ScriptedChild::new("tree", raw_child(), Arc::clone(&calls));
            let mut guard =
                StartingCommand::new("redacted", ManagedChildProcess::new(Box::new(tree), true));
            let finished = Arc::new(AtomicUsize::new(0));
            if with_stdin {
                let (cancel, token) =
                    IoCancellation::pair().expect("stdin cancellation must initialize");
                let finished = Arc::clone(&finished);
                guard.set_stdin_writer(Some(StdinWriter::new(
                    thread::spawn(move || {
                        while !token.is_cancelled() {
                            thread::yield_now();
                        }
                        finished.fetch_add(1, Ordering::SeqCst);
                        Err(io::Error::other("stdin failed"))
                    }),
                    cancel,
                )));
            }
            for stream in 0..readers {
                let (cancel, token) =
                    IoCancellation::pair().expect("output cancellation must initialize");
                let finished = Arc::clone(&finished);
                let reader = OutputReader::new(
                    thread::spawn(move || {
                        while !token.is_cancelled() {
                            thread::yield_now();
                        }
                        finished.fetch_add(1, Ordering::SeqCst);
                        Err(OutputCaptureError::Read {
                            source: io::Error::other("reader failed"),
                            output: CapturedOutput::default(),
                        })
                    }),
                    cancel,
                );
                if stream == 0 {
                    guard.set_stdout_reader(reader);
                } else {
                    guard.set_stderr_reader(reader);
                }
            }
            let error = guard.abort(CommandError::from_reason(
                "redacted",
                CommandErrorReason::TimeFailed {
                    source: TimeError::InstantOverflow,
                },
                None,
            ));
            assert_eq!(error.kind(), CommandErrorKind::TimeFailed);
            assert_eq!(finished.load(Ordering::SeqCst), stage);
            assert_eq!(error.cleanup_failures().len(), stage);
            assert_eq!(
                calls
                    .lock()
                    .expect("calls readable")
                    .iter()
                    .filter(|call| call.as_str() == "tree.kill")
                    .count(),
                1
            );
        }
    }
}
