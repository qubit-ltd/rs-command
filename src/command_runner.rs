// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    fmt,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
    time::Duration,
};

use qubit_clock::{
    MonotonicClock,
    StdMonotonicClock,
    Timer,
};
use qubit_sanitize::{
    FieldSanitizer,
    SensitivityLevel,
};

pub(crate) mod internal;

use internal::error_mapping::{
    output_pipe_error,
    spawn_failed,
};
use internal::finished_command::FinishedCommand;
use internal::output_capture_options::OutputCaptureOptions;
use internal::output_collector::read_output_stream;
use internal::prepared_command::PreparedCommand;
use internal::process_launcher::spawn_child;
use internal::running_command::RunningCommand;
use internal::starting_command::StartingCommand;
use internal::stdin_pipe::write_stdin_bytes;

use crate::{
    Command,
    CommandError,
    CommandOutput,
    OutputStream,
};

const REDACTED_PATH: &str = "<redacted path>";

/// Default ten-second timeout applied by [`CommandRunner::new`].
///
/// Use [`CommandRunner::timeout`] to select a different command limit or
/// [`CommandRunner::without_timeout`] to opt out of timeout handling.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs external commands and captures their output.
///
/// `CommandRunner` runs one [`Command`] synchronously on the caller thread and
/// returns captured process output. The runner always preserves raw output
/// bytes up to the configured per-stream limits. Use
/// [`CommandOutput::stdout_text`] and [`CommandOutput::stderr_text`] for strict
/// UTF-8 text, or [`CommandOutput::stdout_lossy_text`] and
/// [`CommandOutput::stderr_lossy_text`] when invalid UTF-8 should be replaced.
///
/// # Examples
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_command::CommandRunner;
///
/// CommandRunner::new();
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
    /// Field sanitizer used for command diagnostics and logs.
    diagnostic_sanitizer: FieldSanitizer,
    /// Maximum stdout bytes retained in memory.
    max_stdout_bytes: Option<usize>,
    /// Maximum stderr bytes retained in memory.
    max_stderr_bytes: Option<usize>,
    /// File that receives a streaming copy of stdout.
    stdout_file: Option<PathBuf>,
    /// File that receives a streaming copy of stderr.
    stderr_file: Option<PathBuf>,
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
            .field("diagnostic_sanitizer", &self.diagnostic_sanitizer)
            .field("max_stdout_bytes", &self.max_stdout_bytes)
            .field("max_stderr_bytes", &self.max_stderr_bytes)
            .field(
                "stdout_file",
                &self.stdout_file.as_ref().map(|_| REDACTED_PATH),
            )
            .field(
                "stderr_file",
                &self.stderr_file.as_ref().map(|_| REDACTED_PATH),
            )
            .finish()
    }
}

impl Default for CommandRunner {
    /// Creates a command runner with the default exit-code policy.
    ///
    /// # Returns
    ///
    /// A runner with the default timeout, a standard monotonic timer, inherited
    /// working directory, success exit code `0`, enabled logging, unlimited
    /// in-memory output capture, and no output tee files.
    #[inline]
    fn default() -> Self {
        Self {
            timeout: Some(DEFAULT_COMMAND_TIMEOUT),
            timer: StdMonotonicClock::new().new_timer(),
            working_directory: None,
            success_exit_codes: vec![0],
            disable_logging: false,
            diagnostic_sanitizer: FieldSanitizer::default(),
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
    /// A runner with the default timeout, a standard monotonic timer, inherited
    /// working directory, success exit code `0`, enabled logging, unlimited
    /// in-memory output capture, and no output tee files.
    #[inline(always)]
    pub fn new() -> Self {
        Self::default()
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

    /// Sets the command timeout.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Maximum duration allowed for each command.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Disables timeout handling.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub const fn without_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    /// Returns the configured monotonic timer.
    ///
    /// # Returns
    ///
    /// Timer used for timeout measurement and blocking sleeps.
    #[must_use]
    #[inline(always)]
    pub fn configured_timer(&self) -> &dyn Timer {
        self.timer.as_ref()
    }

    /// Replaces the monotonic timer used for timeout handling.
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

    /// Returns whether logging is disabled.
    ///
    /// # Returns
    ///
    /// `true` when runner logs are disabled.
    #[must_use]
    #[inline(always)]
    pub const fn is_logging_disabled(&self) -> bool {
        self.disable_logging
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
    #[inline(always)]
    pub const fn disable_logging(mut self, disable_logging: bool) -> Self {
        self.disable_logging = disable_logging;
        self
    }

    /// Adds one sensitive field name for command diagnostics.
    ///
    /// The field is appended to the default `qubit-sanitize` policy used for
    /// command text in runner logs and [`CommandError::command`]. `Command`'s
    /// standalone [`Debug`](std::fmt::Debug) output has no runner context and
    /// uses the default policy only. Matching uses
    /// [`NameMatchMode::ExactOrSuffix`](qubit_sanitize::NameMatchMode::ExactOrSuffix),
    /// so contextual names such as `TENANT_OPTION` match `tenant_option`.
    ///
    /// # Parameters
    ///
    /// * `field` - Field or option name that should be treated as sensitive.
    /// * `level` - Sensitivity level controlling how values are masked.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn sensitive_field(
        mut self,
        field: &str,
        level: SensitivityLevel,
    ) -> Self {
        self.diagnostic_sanitizer
            .insert_sensitive_field(field, level);
        self
    }

    /// Adds multiple sensitive field names for command diagnostics.
    ///
    /// This is the batch form of [`Self::sensitive_field`]. The fields extend
    /// the default `qubit-sanitize` policy used by runner logs and
    /// [`CommandError::command`]; standalone [`Command`](crate::Command)
    /// [`Debug`](std::fmt::Debug) output still uses only the built-in default
    /// policy because it has no runner context.
    ///
    /// # Parameters
    ///
    /// * `fields` - Field or option names that should be treated as sensitive.
    /// * `level` - Sensitivity level applied to every provided field.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn sensitive_fields(
        mut self,
        fields: &[&str],
        level: SensitivityLevel,
    ) -> Self {
        self.diagnostic_sanitizer
            .extend_sensitive_fields(fields.iter().copied(), level);
        self
    }

    /// Excludes one field from command diagnostic sanitization.
    ///
    /// This can remove a built-in sensitive field. The matching command-line
    /// argument or environment value may then appear verbatim in logs and
    /// [`CommandError::command`]. Use this only for a known false positive
    /// whose exposure is acceptable in the caller's diagnostic context.
    ///
    /// # Parameters
    ///
    /// * `field` - Field or option name to stop treating as sensitive.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub fn exclude_sensitive_field(mut self, field: &str) -> Self {
        self.diagnostic_sanitizer.exclude_sensitive_field(field);
        self
    }

    /// Excludes multiple fields from command diagnostic sanitization.
    ///
    /// This is the batch form of [`Self::exclude_sensitive_field`]. Removed
    /// built-in fields may be rendered verbatim in logs and
    /// [`CommandError::command`], so callers must review every exclusion as a
    /// deliberate disclosure decision.
    ///
    /// # Parameters
    ///
    /// * `fields` - Field or option names to stop treating as sensitive.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    pub fn exclude_sensitive_fields(mut self, fields: &[&str]) -> Self {
        for field in fields {
            self.diagnostic_sanitizer.exclude_sensitive_field(field);
        }
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

    /// Returns the stdout tee file path.
    ///
    /// # Returns
    ///
    /// `Some(path)` when stdout is streamed to a file, otherwise `None`.
    #[inline(always)]
    pub fn configured_stdout_file(&self) -> Option<&Path> {
        self.stdout_file.as_deref()
    }

    /// Streams stdout to a file while still capturing it in memory.
    ///
    /// Before spawning, the file is opened without truncation and checked for
    /// identity conflicts with configured stdin and stderr files. It is
    /// truncated only after all checks pass. Combine this with
    /// [`Self::max_stdout_bytes`] to avoid unbounded memory use for large
    /// stdout streams.
    ///
    /// # Parameters
    ///
    /// * `path` - Destination file path for stdout bytes.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub fn tee_stdout_to_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stdout_file = Some(path.into());
        self
    }

    /// Returns the stderr tee file path.
    ///
    /// # Returns
    ///
    /// `Some(path)` when stderr is streamed to a file, otherwise `None`.
    #[inline(always)]
    pub fn configured_stderr_file(&self) -> Option<&Path> {
        self.stderr_file.as_deref()
    }

    /// Streams stderr to a file while still capturing it in memory.
    ///
    /// Before spawning, the file is opened without truncation and checked for
    /// identity conflicts with configured stdin and stdout files. It is
    /// truncated only after all checks pass. Combine this with
    /// [`Self::max_stderr_bytes`] to avoid unbounded memory use for large
    /// stderr streams.
    ///
    /// # Parameters
    ///
    /// * `path` - Destination file path for stderr bytes.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline(always)]
    pub fn tee_stderr_to_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stderr_file = Some(path.into());
        self
    }

    /// Runs a command and captures stdout and stderr.
    ///
    /// This method blocks the caller thread until the command exits and its I/O
    /// helpers finish, or until the configured timeout is reached. When a
    /// timeout is configured, Unix children run as leaders of new process
    /// groups and Windows children run in Job Objects. This lets timeout
    /// killing target the process tree instead of only the direct child
    /// process, including cases where the direct child exits but descendants
    /// keep inherited stdout or stderr pipes open. Without a configured
    /// timeout, commands use the platform's normal process-spawning
    /// behavior.
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
    /// Returns [`CommandError`] if I/O paths conflict or cannot be prepared,
    /// the process or an I/O helper cannot be started, waiting or injected time
    /// handling fails, the timeout expires, process-tree termination fails,
    /// captured output or configured stdin cannot be transferred, or the child
    /// exits with a code not configured as successful.
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
            &self.diagnostic_sanitizer,
            self.working_directory.as_deref(),
            self.stdout_file.as_deref(),
            self.stderr_file.as_deref(),
        )?;

        if !self.disable_logging {
            log::info!("Running command: {command_text}");
        }

        let child_process =
            match spawn_child(process_command, self.timeout.is_some()) {
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

        let stdout = match starting_command.child_process().stdout().take() {
            Some(stdout) => stdout,
            None => {
                return Err(output_pipe_error(
                    &command_text,
                    OutputStream::Stdout,
                ));
            }
        };
        let stderr = match starting_command.child_process().stderr().take() {
            Some(stderr) => stderr,
            None => {
                return Err(output_pipe_error(
                    &command_text,
                    OutputStream::Stderr,
                ));
            }
        };
        let stdout_reader = read_output_stream(
            Box::new(stdout),
            OutputCaptureOptions::new(
                self.max_stdout_bytes,
                stdout_file,
                stdout_file_path,
            ),
        )
        .map_err(|source| CommandError::StartOutputThreadFailed {
            command: command_text.clone(),
            stream: OutputStream::Stdout,
            source,
        })?;
        starting_command.set_stdout_reader(stdout_reader);
        let stderr_reader = read_output_stream(
            Box::new(stderr),
            OutputCaptureOptions::new(
                self.max_stderr_bytes,
                stderr_file,
                stderr_file_path,
            ),
        )
        .map_err(|source| CommandError::StartOutputThreadFailed {
            command: command_text.clone(),
            stream: OutputStream::Stderr,
            source,
        })?;
        starting_command.set_stderr_reader(stderr_reader);
        if let Err(source) = self.timer.clock().now().duration_since(started_at)
        {
            return Err(CommandError::TimeFailed {
                command: command_text.clone(),
                source,
            });
        }
        let (child_process, command_io) = starting_command.finish();
        let finished = RunningCommand::new(
            command_text,
            child_process,
            command_io,
            started_at,
            Arc::clone(&self.timer),
        )
        .wait_for_completion(self.timeout)?;
        let FinishedCommand {
            command_text,
            output,
        } = finished;

        if output.exit_code().is_some_and(|exit_code| {
            self.success_exit_codes.contains(&exit_code)
        }) {
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
