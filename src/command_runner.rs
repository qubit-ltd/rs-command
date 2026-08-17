// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use qubit_clock::StdTimer;
use qubit_clock::Timer;
use qubit_redact::RedactionPolicy;
use qubit_redact::Redactor;

#[cfg(coverage)]
mod coverage;
mod internal;
#[cfg(coverage)]
#[doc(hidden)]
pub use coverage::__coverage_internal;
use internal::error_mapping::output_pipe_error;
use internal::error_mapping::spawn_failed;
use internal::finished_command::FinishedCommand;
use internal::output_capture_options::OutputCaptureOptions;
use internal::output_collector::read_output_stream;
use internal::output_reader::OutputReader;
use internal::prepared_command::PreparedCommand;
use internal::process_launcher::spawn_child;
use internal::running_command::RunningCommand;
use internal::starting_command::StartingCommand;
use internal::stdin_pipe::write_stdin_bytes;

use crate::Command;
use crate::CommandCancellation;
use crate::CommandError;
use crate::CommandErrorReason;
use crate::CommandOutput;
use crate::CommandRunOptions;
use crate::OutputStream;
use crate::command_run_options_parts::CommandRunOptionsParts;

const REDACTED_PATH: &str = "<redacted path>";

/// Takes a prepared child output pipe and maps an absent pipe to a runner
/// error.
fn take_output_pipe<T>(
    command: &str,
    stream: OutputStream,
    take: impl FnOnce() -> Option<T>,
) -> Result<T, CommandError> {
    take().ok_or_else(|| output_pipe_error(command, stream))
}

/// Starts one output reader and annotates thread-start failures with its
/// stream.
fn start_output_reader(
    command: &str,
    stream: OutputStream,
    start: impl FnOnce() -> io::Result<OutputReader>,
) -> Result<OutputReader, CommandError> {
    start().map_err(|source| {
        CommandError::from_reason(
            command,
            CommandErrorReason::StartOutputThreadFailed { stream, source },
            None,
        )
    })
}

/// Default one-mebibyte in-memory capture limit applied to each output stream.
///
/// Use [`CommandRunner::max_output_bytes`] to select a different per-stream
/// limit or [`CommandRunner::unbounded_output`] to opt out explicitly.
pub const DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM: usize = 1024 * 1024;

/// Runs external commands and captures their output.
///
/// `CommandRunner` runs one [`Command`] synchronously on the caller thread and
/// returns captured process output. The configured timeout begins after the
/// child process is spawned. The runner always preserves raw output bytes up
/// to the configured per-stream limits. By default, each stream is limited to
/// [`DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM`] and successful commands with
/// truncated output are rejected. Use
/// [`CommandRunner::fail_on_output_truncation`] to accept truncated output or
/// [`CommandRunner::unbounded_output`] to remove both limits explicitly. Use
/// [`CommandOutput::stdout_text`] and [`CommandOutput::stderr_text`] for strict
/// UTF-8 text, or [`CommandOutput::stdout_lossy_text`] and
/// [`CommandOutput::stderr_lossy_text`] when invalid UTF-8 should be replaced.
/// Timeout and cancellation handling use a blocking timer adapter, so the
/// configured timer backend must keep progressing while this thread is parked.
///
/// # Examples
///
/// ```rust
/// #![deny(unused_must_use)]
/// use std::time::Duration;
/// use qubit_command::CommandRunner;
///
/// let runner = CommandRunner::new(Duration::from_secs(10));
/// let _ = runner;
/// ```
#[derive(Clone)]
#[must_use]
pub struct CommandRunner {
    /// Maximum duration allowed for each command.
    timeout: Option<Duration>,
    /// Monotonic timer used for timeout measurement and blocking sleeps.
    timer: Arc<dyn Timer>,
    /// Default working directory used when a command does not override it.
    working_directory: Option<PathBuf>,
    /// Exit codes treated as successful.
    success_exit_codes: Vec<i32>,
    /// Whether command execution logs are disabled.
    disable_logging: bool,
    /// Whether successful commands fail when captured output is truncated.
    fail_on_output_truncation: bool,
    /// Immutable redaction policy used for command diagnostics and logs.
    diagnostic_redaction_policy: RedactionPolicy,
    /// Maximum stdout bytes retained in memory.
    max_stdout_bytes: Option<usize>,
    /// Maximum stderr bytes retained in memory.
    max_stderr_bytes: Option<usize>,
}

impl fmt::Debug for CommandRunner {
    /// Formats runner configuration without requiring a debug timer object.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Destination formatter.
    ///
    /// # Returns
    ///
    /// Formatting result after rendering redacted path configuration.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandRunner")
            .field("timeout", &self.timeout)
            .field("timer", &"<dyn Timer>")
            .field(
                "working_directory",
                &self.working_directory.as_ref().map(|_| REDACTED_PATH),
            )
            .field("success_exit_codes", &self.success_exit_codes)
            .field("disable_logging", &self.disable_logging)
            .field("fail_on_output_truncation", &self.fail_on_output_truncation)
            .field(
                "diagnostic_redaction_policy",
                &self.diagnostic_redaction_policy,
            )
            .field("max_stdout_bytes", &self.max_stdout_bytes)
            .field("max_stderr_bytes", &self.max_stderr_bytes)
            .finish()
    }
}

impl CommandRunner {
    /// Creates a command runner with explicit timeout handling configuration.
    #[inline(always)]
    fn with_optional_timeout(timeout: Option<Duration>) -> Self {
        Self {
            timeout,
            timer: Arc::new(StdTimer::new()),
            working_directory: None,
            success_exit_codes: vec![0],
            disable_logging: false,
            fail_on_output_truncation: true,
            diagnostic_redaction_policy: Redactor::default().policy().clone(),
            max_stdout_bytes: Some(DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
            max_stderr_bytes: Some(DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
        }
    }

    /// Creates a command runner with a post-spawn timeout.
    ///
    /// # Returns
    ///
    /// A runner that uses a timeout and inherits default command policies.
    #[inline(always)]
    pub fn new(timeout: Duration) -> Self {
        Self::with_optional_timeout(Some(timeout))
    }

    /// Creates a command runner with timeout handling disabled.
    #[inline(always)]
    pub fn without_timeout() -> Self {
        Self::with_optional_timeout(None)
    }

    /// Returns the configured timeout.
    ///
    /// # Returns
    ///
    /// `Some(duration)` when timeout handling is enabled, otherwise `None`.
    #[inline(always)]
    pub const fn configured_timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Runs a command with default per-run options.
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
    /// Returns [`CommandError`] when command preparation, spawning, waiting,
    /// output collection, timeout handling, cancellation, output truncation,
    /// or exit-status validation fails.
    pub fn run(&self, command: Command) -> Result<CommandOutput, CommandError> {
        self.run_with(command, CommandRunOptions::new())
    }

    /// Runs a command with explicit per-run options.
    ///
    /// # Parameters
    ///
    /// * `command` - Structured command to run.
    /// * `options` - Per-run cancellation and tee destinations.
    ///
    /// # Returns
    ///
    /// Captured output when the process exits with a configured success code.
    ///
    /// # Errors
    ///
    /// Returns a [`CommandError`](crate::CommandError) with kind
    /// [`CommandErrorKind::CancelledBeforeStart`](crate::CommandErrorKind::CancelledBeforeStart)
    /// when a configured
    /// cancellation handle has already been requested before command
    /// preparation, and maps all process, I/O, and timeout failures as
    /// described by [`CommandRunner::run`].
    pub fn run_with(
        &self,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOutput, CommandError> {
        let CommandRunOptionsParts {
            cancellation,
            stdout_file,
            stderr_file,
        } = options.into_parts();

        let prepared = self.prepare_command_for_run(
            command,
            cancellation.as_ref(),
            stdout_file.as_deref(),
            stderr_file.as_deref(),
        )?;
        if cancellation
            .as_ref()
            .is_some_and(CommandCancellation::is_cancelled)
        {
            return Err(CommandError::from_reason(
                prepared.command_text,
                CommandErrorReason::CancelledBeforeStart,
                None,
            ));
        }
        let PreparedCommand {
            command_text,
            process_command,
            stdin_bytes,
            stdout_file,
            stderr_file,
            stdout_file_path,
            stderr_file_path,
            ..
        } = prepared.commit()?;

        if !self.disable_logging {
            log::debug!("Running command: {command_text}");
        }

        let manage_process_tree =
            self.timeout.is_some() || cancellation.is_some();
        let child_process =
            match spawn_child(process_command, manage_process_tree) {
                Ok(child_process) => child_process,
                Err(source) => return Err(spawn_failed(&command_text, source)),
            };
        let mut starting_command =
            StartingCommand::new(&command_text, child_process);
        let started_at = self.timer.clock().now();

        let stdin_writer = write_stdin_bytes(
            &command_text,
            starting_command.child_process(),
            stdin_bytes,
        )?;
        starting_command.set_stdin_writer(stdin_writer);

        let stdout =
            take_output_pipe(&command_text, OutputStream::Stdout, || {
                starting_command.child_process().stdout().take()
            })?;
        let stderr =
            take_output_pipe(&command_text, OutputStream::Stderr, || {
                starting_command.child_process().stderr().take()
            })?;
        let stdout_reader =
            start_output_reader(&command_text, OutputStream::Stdout, || {
                read_output_stream(
                    stdout,
                    OutputCaptureOptions::new(
                        self.max_stdout_bytes,
                        stdout_file,
                        stdout_file_path,
                    ),
                )
            })?;
        starting_command.set_stdout_reader(stdout_reader);
        let stderr_reader =
            start_output_reader(&command_text, OutputStream::Stderr, || {
                read_output_stream(
                    stderr,
                    OutputCaptureOptions::new(
                        self.max_stderr_bytes,
                        stderr_file,
                        stderr_file_path,
                    ),
                )
            })?;
        starting_command.set_stderr_reader(stderr_reader);
        if let Err(source) = self.timer.clock().now().duration_since(started_at)
        {
            return Err(CommandError::from_reason(
                command_text.clone(),
                CommandErrorReason::TimeFailed { source },
                None,
            ));
        }

        let (child_process, command_io) = starting_command.finish();
        let finished = RunningCommand::new(
            command_text,
            child_process,
            command_io,
            started_at,
            Arc::clone(&self.timer),
            cancellation,
        )
        .wait_for_completion(self.timeout)?;
        let FinishedCommand {
            command_text,
            output,
        } = finished;

        if output.exit_code().is_some_and(|exit_code| {
            self.success_exit_codes.contains(&exit_code)
        }) {
            if self.fail_on_output_truncation
                && (output.stdout_truncated() || output.stderr_truncated())
            {
                if !self.disable_logging {
                    log::debug!(
                        "Finished command `{}` with truncated output in {:?}.",
                        command_text,
                        output.elapsed()
                    );
                }
                return Err(CommandError::from_reason(
                    command_text,
                    CommandErrorReason::OutputTruncated,
                    Some(Box::new(output)),
                ));
            }
            if !self.disable_logging {
                log::debug!(
                    "Finished command `{}` in {:?}.",
                    command_text,
                    output.elapsed()
                );
            }
            Ok(output)
        } else {
            if !self.disable_logging {
                log::debug!(
                    "Command `{}` exited with code {:?}.",
                    command_text,
                    output.exit_code()
                );
            }
            Err(CommandError::from_reason(
                command_text,
                CommandErrorReason::UnexpectedExit {
                    exit_code: output.exit_code(),
                    expected: self.success_exit_codes.clone(),
                },
                Some(Box::new(output)),
            ))
        }
    }

    /// Returns the configured monotonic timer.
    ///
    /// # Returns
    ///
    /// Timer used for elapsed measurement and blocking sleeps during timeout or
    /// cancellation handling.
    #[must_use]
    #[inline(always)]
    pub fn configured_timer(&self) -> &dyn Timer {
        self.timer.as_ref()
    }

    /// Replaces the monotonic timer used for elapsed measurement and blocking
    /// sleeps during timeout or cancellation handling.
    ///
    /// Command execution waits on this timer synchronously. Its backend must
    /// therefore make progress independently of the caller thread. In
    /// particular, do not use a Tokio timer whose only driver is the same
    /// current-thread runtime that calls [`Self::run`].
    ///
    /// # Parameters
    ///
    /// * `timer` - Timer whose clock measures elapsed command time.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub fn timer(mut self, timer: Arc<dyn Timer>) -> Self {
        self.timer = timer;
        self
    }

    /// Returns the default working directory.
    ///
    /// # Returns
    ///
    /// `Some(path)` when a default working directory is configured, otherwise
    /// `None` to inherit the current process working directory.
    #[inline(always)]
    pub fn configured_working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    /// Sets the default working directory.
    ///
    /// # Parameters
    ///
    /// * `working_directory` - Directory used when a command has no per-command
    ///   working directory override.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub fn working_directory<P>(mut self, working_directory: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.working_directory = Some(working_directory.into());
        self
    }

    /// Returns the configured successful exit codes.
    ///
    /// # Returns
    ///
    /// Borrowed list of exit codes treated as successful.
    #[must_use]
    #[inline(always)]
    pub fn configured_success_exit_codes(&self) -> &[i32] {
        &self.success_exit_codes
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

    /// Returns whether command lifecycle logging is disabled.
    ///
    /// # Returns
    ///
    /// `true` when runner lifecycle logs are disabled.
    #[must_use]
    #[inline(always)]
    pub const fn is_logging_disabled(&self) -> bool {
        self.disable_logging
    }

    /// Enables or disables command lifecycle logs.
    ///
    /// This setting controls the `debug` records emitted when a command starts
    /// and completes. Cleanup failures that cannot be returned to the caller
    /// may still be logged at `error` level.
    ///
    /// # Parameters
    ///
    /// * `disable_logging` - `true` to suppress runner lifecycle logs.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub const fn disable_logging(mut self, disable_logging: bool) -> Self {
        self.disable_logging = disable_logging;
        self
    }

    /// Returns whether truncated output makes a successful command fail.
    ///
    /// # Returns
    ///
    /// `true` when a successful command returns
    /// [`CommandErrorKind::OutputTruncated`](crate::CommandErrorKind::OutputTruncated) if either captured stream is
    /// truncated.
    #[must_use]
    #[inline(always)]
    pub const fn is_output_truncation_failure_enabled(&self) -> bool {
        self.fail_on_output_truncation
    }

    /// Enables or disables failure on truncated captured output.
    ///
    /// This policy applies only after the process exits with a configured
    /// success code. Timeout and unexpected-exit errors keep precedence and
    /// continue to expose their captured output through
    /// [`CommandError::output`].
    ///
    /// # Parameters
    ///
    /// * `fail_on_output_truncation` - `true` to reject successful commands
    ///   when stdout or stderr is truncated in memory.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub const fn fail_on_output_truncation(
        mut self,
        fail_on_output_truncation: bool,
    ) -> Self {
        self.fail_on_output_truncation = fail_on_output_truncation;
        self
    }

    /// Returns the immutable policy used for command diagnostics and logs.
    ///
    /// # Returns
    ///
    /// The complete configured diagnostic redaction policy.
    #[inline(always)]
    pub const fn configured_diagnostic_redaction_policy(
        &self,
    ) -> &RedactionPolicy {
        &self.diagnostic_redaction_policy
    }

    /// Replaces the complete policy used for command diagnostics and logs.
    ///
    /// The policy affects runner lifecycle logs and
    /// [`CommandError::command`]. Standalone [`Command`](crate::Command)
    /// [`Debug`](std::fmt::Debug) output uses the process-wide global redaction
    /// configuration because it has no runner context.
    ///
    /// # Parameters
    ///
    /// * `policy` - Immutable policy snapshot used for diagnostic redaction.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub fn diagnostic_redaction_policy(
        mut self,
        policy: RedactionPolicy,
    ) -> Self {
        self.diagnostic_redaction_policy = policy;
        self
    }

    /// Returns the configured stdout capture limit.
    ///
    /// # Returns
    ///
    /// `Some(max_bytes)` when stdout capture is limited, otherwise `None`.
    #[inline(always)]
    pub const fn configured_max_stdout_bytes(&self) -> Option<usize> {
        self.max_stdout_bytes
    }

    /// Sets the maximum stdout bytes retained in memory.
    ///
    /// The reader still drains the complete stdout stream. Bytes beyond this
    /// limit are not retained in [`CommandOutput`], but they are still written
    /// to a configured stdout tee file.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Maximum number of stdout bytes to retain.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub const fn max_stdout_bytes(mut self, max_bytes: usize) -> Self {
        self.max_stdout_bytes = Some(max_bytes);
        self
    }

    /// Returns the configured stderr capture limit.
    ///
    /// # Returns
    ///
    /// `Some(max_bytes)` when stderr capture is limited, otherwise `None`.
    #[inline(always)]
    pub const fn configured_max_stderr_bytes(&self) -> Option<usize> {
        self.max_stderr_bytes
    }

    /// Sets the maximum stderr bytes retained in memory.
    ///
    /// The reader still drains the complete stderr stream. Bytes beyond this
    /// limit are not retained in [`CommandOutput`], but they are still written
    /// to a configured stderr tee file.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Maximum number of stderr bytes to retain.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
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
    #[inline(always)]
    pub const fn max_output_bytes(mut self, max_bytes: usize) -> Self {
        self.max_stdout_bytes = Some(max_bytes);
        self.max_stderr_bytes = Some(max_bytes);
        self
    }

    /// Limits both captured streams and rejects any truncated result.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Maximum number of retained bytes for each stream.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub const fn bounded_output(mut self, max_bytes: usize) -> Self {
        self.max_stdout_bytes = Some(max_bytes);
        self.max_stderr_bytes = Some(max_bytes);
        self.fail_on_output_truncation = true;
        self
    }

    /// Removes both in-memory capture limits and accepts complete output.
    ///
    /// This explicit opt-out may allow a child process to consume unbounded
    /// memory through stdout or stderr. Prefer the default finite limits or
    /// [`Self::bounded_output`] unless the command's output volume is trusted.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub const fn unbounded_output(mut self) -> Self {
        self.max_stdout_bytes = None;
        self.max_stderr_bytes = None;
        self.fail_on_output_truncation = false;
        self
    }

    /// Prepares command I/O and diagnostics before spawning the child process.
    ///
    /// # Parameters
    ///
    /// * `command` - Structured command to validate and prepare.
    /// * `cancellation` - Optional cancellation handle checked before setup.
    /// * `stdout_file` - Optional stdout tee path.
    /// * `stderr_file` - Optional stderr tee path.
    ///
    /// # Returns
    ///
    /// A prepared process command and validated I/O resources.
    ///
    /// # Errors
    ///
    /// Returns a preparation or pre-start cancellation error without spawning
    /// the child process.
    fn prepare_command_for_run(
        &self,
        command: Command,
        cancellation: Option<&CommandCancellation>,
        stdout_file: Option<&Path>,
        stderr_file: Option<&Path>,
    ) -> Result<PreparedCommand, CommandError> {
        if cancellation.is_some_and(CommandCancellation::is_cancelled) {
            return Err(CommandError::from_reason(
                command.display_command(&self.diagnostic_redaction_policy),
                CommandErrorReason::CancelledBeforeStart,
                None,
            ));
        }
        PreparedCommand::prepare(
            command,
            &self.diagnostic_redaction_policy,
            self.working_directory.as_deref(),
            stdout_file,
            stderr_file,
        )
    }
}
