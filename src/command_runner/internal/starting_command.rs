// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owns a child process while its I/O helpers are being started.

use process_wrap::std::ChildWrapper;

use super::command_io::CommandIo;
use super::command_io::cancel_and_join_started_helpers;
use super::managed_child_process::ManagedChildProcess;
use super::output_reader::OutputReader;
use super::process_terminator::ProcessTerminator;
use super::stdin_writer::OptionalStdinWriter;
use crate::CommandCleanupFailure;
use crate::CommandError;

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
    pub(in crate::command_runner) const fn new(command: &'a str, child_process: ManagedChildProcess) -> Self {
        Self {
            command,
            child_process: Some(child_process),
            stdout_reader: None,
            stderr_reader: None,
            stdin_writer: None,
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
    #[inline(always)]
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
    #[inline(always)]
    pub(in crate::command_runner) fn set_stdin_writer(&mut self, writer: OptionalStdinWriter) {
        self.stdin_writer = writer;
    }

    /// Records the stdout reader started for this child.
    ///
    /// # Parameters
    ///
    /// * `reader` - Stdout reader helper.
    #[inline(always)]
    pub(in crate::command_runner) fn set_stdout_reader(&mut self, reader: OutputReader) {
        self.stdout_reader = Some(reader);
    }

    /// Records the stderr reader started for this child.
    ///
    /// # Parameters
    ///
    /// * `reader` - Stderr reader helper.
    #[inline(always)]
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
    pub(in crate::command_runner) fn finish(mut self) -> (ManagedChildProcess, CommandIo) {
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
        (
            child_process,
            CommandIo::new(stdout_reader, stderr_reader, stdin_writer),
        )
    }

    /// Explicitly cleans up a failed initialization and preserves its primary
    /// error.
    ///
    /// Cancels all started helpers even when process termination fails. Any
    /// additional failure is attached to the returned error instead of logged.
    pub(in crate::command_runner) fn abort(mut self, primary: CommandError) -> CommandError {
        primary.with_cleanup_failures(self.cleanup())
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
            log::error!("Command '{}' failed during startup cleanup: {failure:?}", self.command);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::Mutex;

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
    fn test_startup_abort_retains_primary_without_waiting_after_failed_kills() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let direct =
            ScriptedChild::new("child", raw_child(), Arc::clone(&calls)).kill_error(io::Error::other("child denied"));
        let tree = ScriptedChild::new("tree", Box::new(direct), Arc::clone(&calls))
            .kill_error(io::Error::other("tree denied"))
            .try_wait_results((0..9).map(|_| Ok(None)));
        let guard = StartingCommand::new("redacted", ManagedChildProcess::new(Box::new(tree), true));
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
        assert_eq!(calls.iter().filter(|call| call.as_str() == "child.kill").count(), 1);
        assert_eq!(calls.iter().filter(|call| call.as_str() == "tree.kill").count(), 1);
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
            let mut guard = StartingCommand::new("redacted", ManagedChildProcess::new(Box::new(tree), true));
            let finished = Arc::new(AtomicUsize::new(0));
            if with_stdin {
                let (cancel, token) = IoCancellation::pair().expect("stdin cancellation must initialize");
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
                let (cancel, token) = IoCancellation::pair().expect("output cancellation must initialize");
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
