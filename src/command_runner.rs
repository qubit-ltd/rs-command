/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::{
    path::{
        Path,
        PathBuf,
    },
    time::Duration,
};

pub(crate) mod captured_output;
pub(crate) mod command_io;
pub(crate) mod error_mapping;
pub(crate) mod finished_command;
pub(crate) mod managed_child_process;
pub(crate) mod output_capture_error;
pub(crate) mod output_capture_options;
pub(crate) mod output_collector;
pub(crate) mod output_reader;
pub(crate) mod output_tee;
pub(crate) mod prepared_command;
pub(crate) mod process_launcher;
pub(crate) mod process_setup;
pub(crate) mod running_command;
pub(crate) mod stdin_pipe;
pub(crate) mod stdin_writer;
pub(crate) mod wait_policy;

use command_io::CommandIo;
use error_mapping::{
    output_pipe_error,
    spawn_failed,
};
use finished_command::FinishedCommand;
use output_capture_options::OutputCaptureOptions;
use output_collector::read_output_stream;
use prepared_command::PreparedCommand;
use process_launcher::spawn_child;
use running_command::RunningCommand;
use stdin_pipe::write_stdin_bytes;

use crate::{
    Command,
    CommandError,
    CommandOutput,
    OutputStream,
};

/// Predefined ten-second timeout value.
///
/// `CommandRunner::new` does not apply this timeout automatically. Use this
/// constant with [`CommandRunner::timeout`] when callers want a short, explicit
/// command limit.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs external commands and captures their output.
///
/// `CommandRunner` runs one [`Command`] synchronously on the caller thread and
/// returns captured process output. The runner always preserves raw output
/// bytes. Its lossy-output option controls whether [`CommandOutput::stdout`]
/// and [`CommandOutput::stderr`] reject invalid UTF-8 or return replacement
/// characters.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRunner {
    /// Maximum duration allowed for each command.
    timeout: Option<Duration>,
    /// Default working directory used when a command does not override it.
    working_directory: Option<PathBuf>,
    /// Exit codes treated as successful.
    success_exit_codes: Vec<i32>,
    /// Whether command execution logs are disabled.
    disable_logging: bool,
    /// Maximum stdout bytes retained in memory.
    max_stdout_bytes: Option<usize>,
    /// Maximum stderr bytes retained in memory.
    max_stderr_bytes: Option<usize>,
    /// File that receives a streaming copy of stdout.
    stdout_file: Option<PathBuf>,
    /// File that receives a streaming copy of stderr.
    stderr_file: Option<PathBuf>,
}

impl Default for CommandRunner {
    /// Creates a command runner with the default exit-code policy.
    ///
    /// # Returns
    ///
    /// A runner with no timeout, inherited working directory, success exit code
    /// `0`, unlimited in-memory output capture, and no output tee files.
    #[inline]
    fn default() -> Self {
        Self {
            timeout: None,
            working_directory: None,
            success_exit_codes: vec![0],
            disable_logging: false,
            max_stdout_bytes: None,
            max_stderr_bytes: None,
            stdout_file: None,
            stderr_file: None,
        }
    }
}

impl CommandRunner {
    /// Creates a command runner with default settings.
    ///
    /// # Returns
    ///
    /// A runner with no timeout, inherited working directory, success exit code
    /// `0`, unlimited in-memory output capture, and no output tee files.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the command timeout.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Maximum duration allowed for each command.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Disables timeout handling.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn without_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    /// Sets the default working directory.
    ///
    /// # Parameters
    ///
    /// * `working_directory` - Directory used when a command has no
    ///   per-command working directory override.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn working_directory<P>(mut self, working_directory: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.working_directory = Some(working_directory.into());
        self
    }

    /// Sets the only exit code treated as successful.
    ///
    /// # Parameters
    ///
    /// * `exit_code` - Exit code considered successful.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn success_exit_code(mut self, exit_code: i32) -> Self {
        self.success_exit_codes = vec![exit_code];
        self
    }

    /// Sets all exit codes treated as successful.
    ///
    /// # Parameters
    ///
    /// * `exit_codes` - Exit codes considered successful.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn success_exit_codes(mut self, exit_codes: &[i32]) -> Self {
        self.success_exit_codes = exit_codes.to_vec();
        self
    }

    /// Enables or disables command execution logs.
    ///
    /// # Parameters
    ///
    /// * `disable_logging` - `true` to suppress runner logs.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn disable_logging(mut self, disable_logging: bool) -> Self {
        self.disable_logging = disable_logging;
        self
    }

    /// Sets the maximum stdout bytes retained in memory.
    ///
    /// The reader still drains the complete stdout stream. Bytes beyond this
    /// limit are not retained in [`CommandOutput`], but they are still written to
    /// a configured stdout tee file.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Maximum number of stdout bytes to retain.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn max_stdout_bytes(mut self, max_bytes: usize) -> Self {
        self.max_stdout_bytes = Some(max_bytes);
        self
    }

    /// Sets the maximum stderr bytes retained in memory.
    ///
    /// The reader still drains the complete stderr stream. Bytes beyond this
    /// limit are not retained in [`CommandOutput`], but they are still written to
    /// a configured stderr tee file.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Maximum number of stderr bytes to retain.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn max_stderr_bytes(mut self, max_bytes: usize) -> Self {
        self.max_stderr_bytes = Some(max_bytes);
        self
    }

    /// Sets the same in-memory capture limit for stdout and stderr.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Maximum number of bytes retained for each stream.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn max_output_bytes(mut self, max_bytes: usize) -> Self {
        self.max_stdout_bytes = Some(max_bytes);
        self.max_stderr_bytes = Some(max_bytes);
        self
    }

    /// Streams stdout to a file while still capturing it in memory.
    ///
    /// The file is created or truncated before the command is spawned. Combine
    /// this with [`Self::max_stdout_bytes`] to avoid unbounded memory use for
    /// large stdout streams.
    ///
    /// # Parameters
    ///
    /// * `path` - Destination file path for stdout bytes.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn tee_stdout_to_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stdout_file = Some(path.into());
        self
    }

    /// Streams stderr to a file while still capturing it in memory.
    ///
    /// The file is created or truncated before the command is spawned. Combine
    /// this with [`Self::max_stderr_bytes`] to avoid unbounded memory use for
    /// large stderr streams.
    ///
    /// # Parameters
    ///
    /// * `path` - Destination file path for stderr bytes.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn tee_stderr_to_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stderr_file = Some(path.into());
        self
    }

    /// Returns the configured timeout.
    ///
    /// # Returns
    ///
    /// `Some(duration)` when timeout handling is enabled, otherwise `None`.
    #[inline]
    pub const fn configured_timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Returns the default working directory.
    ///
    /// # Returns
    ///
    /// `Some(path)` when a default working directory is configured, otherwise
    /// `None` to inherit the current process working directory.
    #[inline]
    pub fn configured_working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    /// Returns the configured successful exit codes.
    ///
    /// # Returns
    ///
    /// Borrowed list of exit codes treated as successful.
    #[inline]
    pub fn configured_success_exit_codes(&self) -> &[i32] {
        &self.success_exit_codes
    }

    /// Returns whether logging is disabled.
    ///
    /// # Returns
    ///
    /// `true` when runner logs are disabled.
    #[inline]
    pub const fn is_logging_disabled(&self) -> bool {
        self.disable_logging
    }

    /// Returns the configured stdout capture limit.
    ///
    /// # Returns
    ///
    /// `Some(max_bytes)` when stdout capture is limited, otherwise `None`.
    #[inline]
    pub const fn configured_max_stdout_bytes(&self) -> Option<usize> {
        self.max_stdout_bytes
    }

    /// Returns the configured stderr capture limit.
    ///
    /// # Returns
    ///
    /// `Some(max_bytes)` when stderr capture is limited, otherwise `None`.
    #[inline]
    pub const fn configured_max_stderr_bytes(&self) -> Option<usize> {
        self.max_stderr_bytes
    }

    /// Returns the stdout tee file path.
    ///
    /// # Returns
    ///
    /// `Some(path)` when stdout is streamed to a file, otherwise `None`.
    #[inline]
    pub fn configured_stdout_file(&self) -> Option<&Path> {
        self.stdout_file.as_deref()
    }

    /// Returns the stderr tee file path.
    ///
    /// # Returns
    ///
    /// `Some(path)` when stderr is streamed to a file, otherwise `None`.
    #[inline]
    pub fn configured_stderr_file(&self) -> Option<&Path> {
        self.stderr_file.as_deref()
    }

    /// Runs a command and captures stdout and stderr.
    ///
    /// This method blocks the caller thread until the child process exits or
    /// the configured timeout is reached. When a timeout is configured, Unix
    /// children run as leaders of new process groups and Windows children run
    /// in Job Objects. This lets timeout killing target the process tree
    /// instead of only the direct child process. Without a configured timeout,
    /// commands use the platform's normal process-spawning behavior.
    ///
    /// Captured output is retained as raw bytes up to the configured per-stream
    /// limits. Reader threads still drain complete streams so the child is not
    /// blocked on full pipes. Use [`CommandOutput::stdout_text`] and
    /// [`CommandOutput::stderr_text`] for strict UTF-8 text, or
    /// [`CommandOutput::stdout_lossy_text`] and
    /// [`CommandOutput::stderr_lossy_text`] when invalid UTF-8 should be
    /// replaced.
    ///
    /// # Parameters
    ///
    /// * `command` - Structured command to run.
    ///
    /// # Returns
    ///
    /// Captured output when the process exits with a configured success code.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if the process cannot be spawned, cannot be
    /// waited on, times out, cannot be killed after timing out, emits output
    /// that cannot be read or written to a tee file, cannot receive configured
    /// stdin, or exits with a code not configured as successful.
    pub fn run(&self, command: Command) -> Result<CommandOutput, CommandError> {
        let PreparedCommand {
            command_text,
            process_command,
            stdin_bytes,
            stdout_file,
            stderr_file,
            stdout_file_path,
            stderr_file_path,
        } = PreparedCommand::prepare(
            command,
            self.working_directory.as_deref(),
            self.stdout_file.as_deref(),
            self.stderr_file.as_deref(),
        )?;

        if !self.disable_logging {
            log::info!("Running command: {command_text}");
        }

        let mut child_process = match spawn_child(process_command, self.timeout.is_some()) {
            Ok(child_process) => child_process,
            Err(source) => return Err(spawn_failed(&command_text, source)),
        };

        let stdin_writer = write_stdin_bytes(&command_text, child_process.as_mut(), stdin_bytes)?;

        let stdout = match child_process.stdout().take() {
            Some(stdout) => stdout,
            None => return Err(output_pipe_error(&command_text, OutputStream::Stdout)),
        };
        let stderr = match child_process.stderr().take() {
            Some(stderr) => stderr,
            None => return Err(output_pipe_error(&command_text, OutputStream::Stderr)),
        };
        let stdout_reader = read_output_stream(
            Box::new(stdout),
            OutputCaptureOptions::new(self.max_stdout_bytes, stdout_file, stdout_file_path),
        );
        let stderr_reader = read_output_stream(
            Box::new(stderr),
            OutputCaptureOptions::new(self.max_stderr_bytes, stderr_file, stderr_file_path),
        );
        let command_io = CommandIo::new(stdout_reader, stderr_reader, stdin_writer);
        let finished = RunningCommand::new(command_text, child_process, command_io)
            .wait_for_completion(self.timeout)?;
        let FinishedCommand {
            command_text,
            output,
        } = finished;

        if output
            .exit_code()
            .is_some_and(|exit_code| self.success_exit_codes.contains(&exit_code))
        {
            if !self.disable_logging {
                log::info!(
                    "Finished command `{}` in {:?}.",
                    command_text,
                    output.elapsed()
                );
            }
            Ok(output)
        } else {
            if !self.disable_logging {
                log::error!(
                    "Command `{}` exited with code {:?}.",
                    command_text,
                    output.exit_code()
                );
            }
            Err(CommandError::UnexpectedExit {
                command: command_text,
                exit_code: output.exit_code(),
                expected: self.success_exit_codes.clone(),
                output: Box::new(output),
            })
        }
    }
}
