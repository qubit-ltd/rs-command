/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    io::{
        self,
        Read,
    },
    path::{
        Path,
        PathBuf,
    },
    process::{
        ChildStderr,
        ChildStdout,
        Command,
        Stdio,
    },
    thread,
    time::{
        Duration,
        Instant,
    },
};

use crate::{
    CommandError,
    CommandOutput,
    CommandSpec,
    OutputStream,
};

/// Default command execution timeout.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Polling interval used while waiting for a child process with timeout.
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Runs external commands and captures their output.
///
/// `CommandRunner` is a process-running utility, not an implementation of a
/// generic task executor. It runs one [`CommandSpec`] synchronously on the
/// caller thread and returns captured process output.
///
/// # Author
///
/// Haixing Hu
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
}

impl Default for CommandRunner {
    /// Creates a command runner with the default timeout and exit-code policy.
    ///
    /// # Returns
    ///
    /// A runner with a 10-second timeout, inherited working directory, and
    /// success exit code `0`.
    #[inline]
    fn default() -> Self {
        Self {
            timeout: Some(DEFAULT_COMMAND_TIMEOUT),
            working_directory: None,
            success_exit_codes: vec![0],
            disable_logging: false,
        }
    }
}

impl CommandRunner {
    /// Creates a command runner with default settings.
    ///
    /// # Returns
    ///
    /// A runner with a 10-second timeout, inherited working directory, and
    /// success exit code `0`.
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

    /// Runs a command and captures stdout and stderr.
    ///
    /// This method blocks the caller thread until the child process exits or
    /// the configured timeout is reached. Captured output is retained as raw
    /// bytes.
    ///
    /// # Parameters
    ///
    /// * `spec` - Structured command specification to run.
    ///
    /// # Returns
    ///
    /// Captured output when the process exits with a configured success code.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if the process cannot be spawned, cannot be
    /// waited on, times out, cannot be killed after timing out, emits output
    /// that cannot be read, or exits with a code not configured as successful.
    pub fn run(&self, spec: CommandSpec) -> Result<CommandOutput, CommandError> {
        let command_text = spec.display_command();
        if !self.disable_logging {
            log::info!("Running command: {command_text}");
        }

        let mut command = Command::new(spec.program());
        command.args(spec.arguments());
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        if let Some(working_directory) = spec
            .working_directory_override()
            .or(self.working_directory.as_deref())
        {
            command.current_dir(working_directory);
        }

        for (key, value) in spec.environment() {
            command.env(key, value);
        }

        let mut child = command
            .spawn()
            .map_err(|source| CommandError::SpawnFailed {
                command: command_text.clone(),
                source,
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| output_pipe_error(&command_text, OutputStream::Stdout))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| output_pipe_error(&command_text, OutputStream::Stderr))?;
        let stdout_reader = read_stdout(stdout);
        let stderr_reader = read_stderr(stderr);

        let start = Instant::now();
        let exit_status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|source| CommandError::WaitFailed {
                    command: command_text.clone(),
                    source,
                })?
            {
                break status;
            }
            if let Some(timeout) = self.timeout
                && start.elapsed() >= timeout
            {
                if let Err(source) = child.kill() {
                    return Err(CommandError::KillFailed {
                        command: command_text,
                        timeout,
                        source,
                    });
                }
                let exit_status = child.wait().map_err(|source| CommandError::WaitFailed {
                    command: command_text.clone(),
                    source,
                })?;
                let output = collect_output(
                    &command_text,
                    exit_status.code(),
                    start.elapsed(),
                    stdout_reader,
                    stderr_reader,
                )?;
                return Err(CommandError::TimedOut {
                    command: command_text,
                    timeout,
                    output: Box::new(output),
                });
            }
            thread::sleep(next_sleep(self.timeout, start.elapsed()));
        };

        let output = collect_output(
            &command_text,
            exit_status.code(),
            start.elapsed(),
            stdout_reader,
            stderr_reader,
        )?;

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

/// Spawns a reader thread for stdout.
///
/// # Parameters
///
/// * `stdout` - Child process stdout pipe.
///
/// # Returns
///
/// Join handle resolving to captured stdout bytes.
fn read_stdout(stdout: ChildStdout) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || read_all(stdout))
}

/// Spawns a reader thread for stderr.
///
/// # Parameters
///
/// * `stderr` - Child process stderr pipe.
///
/// # Returns
///
/// Join handle resolving to captured stderr bytes.
fn read_stderr(stderr: ChildStderr) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || read_all(stderr))
}

/// Reads all bytes from a child output stream.
///
/// # Parameters
///
/// * `reader` - Pipe reader to drain.
///
/// # Returns
///
/// All bytes read from the pipe.
///
/// # Errors
///
/// Returns the I/O error reported by [`Read::read_to_end`].
fn read_all<R>(mut reader: R) -> io::Result<Vec<u8>>
where
    R: Read,
{
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// Collects reader-thread results into a command output value.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `exit_code` - Process exit code, if available.
/// * `elapsed` - Observed command duration.
/// * `stdout_reader` - Reader thread for stdout.
/// * `stderr_reader` - Reader thread for stderr.
///
/// # Returns
///
/// Command output containing both captured streams.
///
/// # Errors
///
/// Returns [`CommandError::ReadOutputFailed`] if either stream cannot be read
/// or if a reader thread panics.
fn collect_output(
    command: &str,
    exit_code: Option<i32>,
    elapsed: Duration,
    stdout_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    stderr_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<CommandOutput, CommandError> {
    let stdout = join_output_reader(command, OutputStream::Stdout, stdout_reader)?;
    let stderr = join_output_reader(command, OutputStream::Stderr, stderr_reader)?;
    Ok(CommandOutput::new(exit_code, stdout, stderr, elapsed))
}

/// Joins one output reader and maps failures to command errors.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `stream` - Stream associated with the reader.
/// * `reader` - Join handle to collect.
///
/// # Returns
///
/// Captured bytes for the requested stream.
///
/// # Errors
///
/// Returns [`CommandError::ReadOutputFailed`] when the reader reports I/O
/// failure or panics.
fn join_output_reader(
    command: &str,
    stream: OutputStream,
    reader: thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, CommandError> {
    match reader.join() {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(source)) => Err(CommandError::ReadOutputFailed {
            command: command.to_owned(),
            stream,
            source,
        }),
        Err(_) => Err(CommandError::ReadOutputFailed {
            command: command.to_owned(),
            stream,
            source: io::Error::other("output reader thread panicked"),
        }),
    }
}

/// Builds an internal missing-pipe error.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `stream` - Missing output stream.
///
/// # Returns
///
/// A command error describing the missing pipe.
fn output_pipe_error(command: &str, stream: OutputStream) -> CommandError {
    CommandError::ReadOutputFailed {
        command: command.to_owned(),
        stream,
        source: io::Error::other(format!("{} pipe was not created", stream.as_str())),
    }
}

/// Calculates how long to sleep before polling the child again.
///
/// # Parameters
///
/// * `timeout` - Optional command timeout.
/// * `elapsed` - Elapsed command duration.
///
/// # Returns
///
/// A short polling delay that does not intentionally sleep past the timeout.
fn next_sleep(timeout: Option<Duration>, elapsed: Duration) -> Duration {
    if let Some(timeout) = timeout
        && let Some(remaining) = timeout.checked_sub(elapsed)
    {
        return remaining.min(WAIT_POLL_INTERVAL);
    }
    WAIT_POLL_INTERVAL
}
