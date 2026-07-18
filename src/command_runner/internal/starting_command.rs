// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owns a child process while its I/O helpers are being started.

use process_wrap::std::ChildWrapper;

use super::{
    command_io::CommandIo,
    managed_child_process::ManagedChildProcess,
    output_reader::OutputReader,
    stdin_writer::StdinWriter,
};

/// Guards a spawned child until all runner-side I/O helpers are ready.
///
/// Dropping an unfinished guard performs best-effort process termination and
/// joins already-started helpers after the child is confirmed stopped.
#[must_use = "dropping an unfinished command guard terminates the child"]
pub(crate) struct StartingCommand<'a> {
    /// Sanitized command text used in cleanup logs.
    command: &'a str,
    /// Spawned child process, moved out only after successful initialization.
    child_process: Option<ManagedChildProcess>,
    /// Started stdout reader, if initialization reached that stage.
    stdout_reader: Option<OutputReader>,
    /// Started stderr reader, if initialization reached that stage.
    stderr_reader: Option<OutputReader>,
    /// Optional started stdin writer.
    stdin_writer: StdinWriter,
}

impl<'a> StartingCommand<'a> {
    /// Takes ownership of a newly spawned child process.
    ///
    /// # Parameters
    ///
    /// * `command` - Sanitized command text used in cleanup logs.
    /// * `child_process` - Newly spawned child process.
    ///
    /// # Returns
    ///
    /// A guard that terminates the child unless initialization finishes.
    #[inline]
    pub(crate) const fn new(
        command: &'a str,
        child_process: ManagedChildProcess,
    ) -> Self {
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
    pub(crate) fn child_process(&mut self) -> &mut dyn ChildWrapper {
        self.child_process
            .as_deref_mut()
            .expect("a starting command always owns its child")
    }

    /// Records the optional stdin writer started for this child.
    ///
    /// # Parameters
    ///
    /// * `writer` - Optional stdin writer helper.
    #[inline(always)]
    pub(crate) fn set_stdin_writer(&mut self, writer: StdinWriter) {
        self.stdin_writer = writer;
    }

    /// Records the stdout reader started for this child.
    ///
    /// # Parameters
    ///
    /// * `reader` - Stdout reader helper.
    #[inline(always)]
    pub(crate) fn set_stdout_reader(&mut self, reader: OutputReader) {
        self.stdout_reader = Some(reader);
    }

    /// Records the stderr reader started for this child.
    ///
    /// # Parameters
    ///
    /// * `reader` - Stderr reader helper.
    #[inline(always)]
    pub(crate) fn set_stderr_reader(&mut self, reader: OutputReader) {
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
    pub(crate) fn finish(mut self) -> (ManagedChildProcess, CommandIo) {
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

    /// Joins all helper threads after the child is confirmed stopped.
    ///
    /// This method blocks until every registered helper finishes.
    fn join_helpers(&mut self) {
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
        if let Some(writer) = self.stdin_writer.take() {
            let _ = writer.join();
        }
    }
}

impl Drop for StartingCommand<'_> {
    /// Best-effort cleanup for initialization that returns early.
    fn drop(&mut self) {
        let Some(mut child_process) = self.child_process.take() else {
            return;
        };
        let child_stopped = match child_process.start_kill() {
            Ok(()) => match child_process.wait() {
                Ok(_) => true,
                Err(source) => {
                    log::error!(
                        "Failed to wait for command '{}' during startup cleanup: {}",
                        self.command,
                        source,
                    );
                    false
                }
            },
            Err(kill_source) => match child_process.try_wait() {
                Ok(Some(_)) => true,
                Ok(None) => {
                    log::error!(
                        "Failed to kill command '{}' during startup cleanup: {}",
                        self.command,
                        kill_source,
                    );
                    false
                }
                Err(wait_source) => {
                    log::error!(
                        "Failed to kill or inspect command '{}' during startup cleanup: {}; {}",
                        self.command,
                        kill_source,
                        wait_source,
                    );
                    false
                }
            },
        };
        if child_stopped {
            self.join_helpers();
        }
    }
}
