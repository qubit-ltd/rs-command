/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    fs::File,
    io::{
        self,
        Read,
        Write,
    },
    path::{
        Path,
        PathBuf,
    },
    process::{
        Command as ProcessCommand,
        ExitStatus,
        Stdio,
    },
    thread,
    time::Duration,
};

#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{
    ChildWrapper,
    CommandWrap,
};

pub(crate) mod captured_output;
pub(crate) mod command_io;
pub(crate) mod finished_command;
pub(crate) mod managed_child_process;
pub(crate) mod output_capture_error;
pub(crate) mod output_capture_options;
pub(crate) mod output_reader;
pub(crate) mod output_tee;
pub(crate) mod running_command;
pub(crate) mod stdin_writer;

use captured_output::CapturedOutput;
use command_io::CommandIo;
use finished_command::FinishedCommand;
use managed_child_process::ManagedChildProcess;
use output_capture_error::OutputCaptureError;
use output_capture_options::OutputCaptureOptions;
use output_reader::OutputReader;
use running_command::RunningCommand;
use stdin_writer::StdinWriter;

use crate::command_stdin::CommandStdin;
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

/// Polling interval used while waiting for a child process with timeout.
pub(crate) const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Runs external commands and captures their output.
///
/// `CommandRunner` runs one [`Command`] synchronously on the caller thread and
/// returns captured process output. The runner always preserves raw output
/// bytes. Its lossy-output option controls whether [`CommandOutput::stdout`]
/// and [`CommandOutput::stderr`] reject invalid UTF-8 or return replacement
/// characters.
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
    /// Whether captured text accessors should replace invalid UTF-8 bytes.
    lossy_output: bool,
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
    /// `0`, strict UTF-8 output text accessors, unlimited in-memory output
    /// capture, and no output tee files.
    #[inline]
    fn default() -> Self {
        Self {
            timeout: None,
            working_directory: None,
            success_exit_codes: vec![0],
            disable_logging: false,
            lossy_output: false,
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
    /// `0`, strict UTF-8 output text accessors, unlimited in-memory output
    /// capture, and no output tee files.
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

    /// Configures whether output text accessors use lossy UTF-8 conversion.
    ///
    /// # Parameters
    ///
    /// * `lossy_output` - `true` to replace invalid UTF-8 bytes with the
    ///   Unicode replacement character when [`CommandOutput::stdout`] or
    ///   [`CommandOutput::stderr`] is called.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn lossy_output(mut self, lossy_output: bool) -> Self {
        self.lossy_output = lossy_output;
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

    /// Returns whether output text accessors use lossy UTF-8 conversion.
    ///
    /// # Returns
    ///
    /// `true` when invalid UTF-8 bytes are replaced before output is returned
    /// by [`CommandOutput::stdout`] or [`CommandOutput::stderr`].
    #[inline]
    pub const fn is_lossy_output_enabled(&self) -> bool {
        self.lossy_output
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
    /// blocked on full pipes. If lossy output mode is enabled, invalid UTF-8 is
    /// replaced only for [`CommandOutput::stdout`] and
    /// [`CommandOutput::stderr`]; byte accessors still return the retained raw
    /// process output.
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
        let command_text = command.display_command();
        if !self.disable_logging {
            log::info!("Running command: {command_text}");
        }

        let mut process_command = ProcessCommand::new(command.program());
        process_command.args(command.arguments());
        process_command.stdout(Stdio::piped());
        process_command.stderr(Stdio::piped());

        if let Some(working_directory) = command
            .working_directory_override()
            .or(self.working_directory.as_deref())
        {
            process_command.current_dir(working_directory);
        }

        configure_environment(&command, &mut process_command);
        let stdin_configuration = command.into_stdin_configuration();
        let stdin_bytes =
            configure_stdin(&command_text, stdin_configuration, &mut process_command)?;

        let stdout_file = open_output_file(
            &command_text,
            OutputStream::Stdout,
            self.stdout_file.as_deref(),
        )?;
        let stderr_file = open_output_file(
            &command_text,
            OutputStream::Stderr,
            self.stderr_file.as_deref(),
        )?;

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
            OutputCaptureOptions::new(self.max_stdout_bytes, stdout_file, self.stdout_file.clone()),
        );
        let stderr_reader = read_output_stream(
            Box::new(stderr),
            OutputCaptureOptions::new(self.max_stderr_bytes, stderr_file, self.stderr_file.clone()),
        );
        let command_io = CommandIo::new(stdout_reader, stderr_reader, stdin_writer);
        let finished =
            RunningCommand::new(command_text, child_process, command_io, self.lossy_output)
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

/// Configures stdin for a process command.
///
/// # Parameters
///
/// * `command_text` - Human-readable command text for diagnostics.
/// * `stdin` - Owned stdin configuration for the command.
/// * `process_command` - Process command being prepared.
///
/// # Returns
///
/// `Some(bytes)` when stdin bytes must be written after spawning, otherwise
/// `None`.
///
/// # Errors
///
/// Returns [`CommandError::OpenInputFailed`] when a configured stdin file cannot
/// be opened.
fn configure_stdin(
    command_text: &str,
    stdin: CommandStdin,
    process_command: &mut ProcessCommand,
) -> Result<Option<Vec<u8>>, CommandError> {
    match stdin {
        CommandStdin::Null => {
            process_command.stdin(Stdio::null());
            Ok(None)
        }
        CommandStdin::Inherit => {
            process_command.stdin(Stdio::inherit());
            Ok(None)
        }
        CommandStdin::Bytes(bytes) => {
            process_command.stdin(Stdio::piped());
            Ok(Some(bytes))
        }
        CommandStdin::File(path) => match File::open(&path) {
            Ok(file) => {
                process_command.stdin(Stdio::from(file));
                Ok(None)
            }
            Err(source) => Err(CommandError::OpenInputFailed {
                command: command_text.to_owned(),
                path,
                source,
            }),
        },
    }
}

/// Configures environment variables for a process command.
///
/// # Parameters
///
/// * `command` - Command environment configuration.
/// * `process_command` - Process command being prepared.
fn configure_environment(command: &Command, process_command: &mut ProcessCommand) {
    if command.clears_environment() {
        process_command.env_clear();
    }
    for key in command.removed_environment() {
        process_command.env_remove(key);
    }
    for (key, value) in command.environment() {
        process_command.env(key, value);
    }
}

/// Spawns a child process with platform process-tree support.
///
/// # Parameters
///
/// * `process_command` - Prepared standard-library process command.
/// * `kill_process_tree` - Whether timeout handling needs process-tree
///   termination support.
///
/// # Returns
///
/// A child process. When `kill_process_tree` is `true`, Unix children are
/// placed in a new process group and Windows children are placed in a Job
/// Object.
///
/// # Errors
///
/// Returns the I/O error reported by process spawning or wrapper setup.
fn spawn_child(
    process_command: ProcessCommand,
    kill_process_tree: bool,
) -> io::Result<ManagedChildProcess> {
    #[cfg(coverage)]
    if crate::coverage_support::fake_children_enabled()
        && let Some(child) = crate::coverage_support::fake_child_for(process_command.get_program())
    {
        return Ok(child);
    }

    let mut command = CommandWrap::from(process_command);
    #[cfg(unix)]
    if kill_process_tree {
        command.wrap(ProcessGroup::leader());
    }
    #[cfg(windows)]
    if kill_process_tree {
        command.wrap(JobObject);
    }
    command.spawn()
}

/// Opens an output tee file before spawning the child.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `stream` - Stream associated with the file.
/// * `path` - Optional file path.
///
/// # Returns
///
/// Open file handle when a path is configured, otherwise `None`.
///
/// # Errors
///
/// Returns [`CommandError::OpenOutputFailed`] when the path cannot be opened.
fn open_output_file(
    command: &str,
    stream: OutputStream,
    path: Option<&Path>,
) -> Result<Option<File>, CommandError> {
    match path {
        Some(path) => {
            File::create(path)
                .map(Some)
                .map_err(|source| CommandError::OpenOutputFailed {
                    command: command.to_owned(),
                    stream,
                    path: path.to_path_buf(),
                    source,
                })
        }
        None => Ok(None),
    }
}

/// Starts a helper thread that writes configured stdin bytes.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `child` - Spawned child process wrapper.
/// * `stdin_bytes` - Optional bytes to write to stdin.
///
/// # Returns
///
/// Join handle for the stdin writer, if stdin bytes were configured.
///
/// # Errors
///
/// Returns [`CommandError::WriteInputFailed`] if stdin bytes were configured but
/// the child stdin pipe was not available.
pub(crate) fn write_stdin_bytes(
    command: &str,
    child: &mut dyn ChildWrapper,
    stdin_bytes: Option<Vec<u8>>,
) -> Result<StdinWriter, CommandError> {
    match stdin_bytes {
        Some(bytes) => match child.stdin().take() {
            Some(mut stdin) => Ok(Some(thread::spawn(move || stdin.write_all(&bytes)))),
            None => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source: io::Error::other("stdin pipe was not created"),
            }),
        },
        None => Ok(None),
    }
}

/// Spawns a reader thread for a child output stream.
///
/// # Parameters
///
/// * `reader` - Child process output pipe.
/// * `options` - Capture and tee-file options.
///
/// # Returns
///
/// Join handle resolving to captured output bytes and truncation metadata.
fn read_output_stream(
    mut reader: Box<dyn Read + Send>,
    options: OutputCaptureOptions,
) -> OutputReader {
    thread::spawn(move || read_output(reader.as_mut(), options))
}

/// Reads one child output stream to completion.
///
/// # Parameters
///
/// * `reader` - Pipe reader to drain.
/// * `options` - Capture and tee-file options.
///
/// # Returns
///
/// Captured bytes and truncation metadata.
///
/// # Errors
///
/// Returns [`OutputCaptureError`] if reading the pipe or writing the tee file
/// fails. Tee-file write failures are recorded while the reader continues
/// draining the child pipe so the child is not blocked by a full output pipe.
pub(crate) fn read_output(
    reader: &mut dyn Read,
    mut options: OutputCaptureOptions,
) -> Result<CapturedOutput, OutputCaptureError> {
    let mut bytes = Vec::new();
    if let Some(max_bytes) = options.max_bytes {
        bytes.reserve(max_bytes.min(8 * 1024));
    }
    let mut truncated = false;
    let mut write_error = None;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(OutputCaptureError::Read)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if let Some(tee) = options.tee.as_mut()
            && write_error.is_none()
            && let Err(source) = tee.writer.write_all(chunk)
        {
            write_error = Some(OutputCaptureError::Write {
                path: tee.path.clone(),
                source,
            });
            options.tee = None;
        }
        match options.max_bytes {
            Some(max_bytes) => {
                let remaining = max_bytes.saturating_sub(bytes.len());
                if remaining > 0 {
                    let retained = remaining.min(chunk.len());
                    bytes.extend_from_slice(&chunk[..retained]);
                }
                if chunk.len() > remaining {
                    truncated = true;
                }
            }
            None => bytes.extend_from_slice(chunk),
        }
    }
    if write_error.is_none()
        && let Some(tee) = options.tee.as_mut()
        && let Err(source) = tee.writer.flush()
    {
        write_error = Some(OutputCaptureError::Write {
            path: tee.path.clone(),
            source,
        });
    }
    if let Some(error) = write_error {
        Err(error)
    } else {
        Ok(CapturedOutput { bytes, truncated })
    }
}

/// Builds a process spawn failure.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `source` - I/O error reported by process spawning.
///
/// # Returns
///
/// A command error preserving the command text and source error.
pub(crate) fn spawn_failed(command: &str, source: io::Error) -> CommandError {
    CommandError::SpawnFailed {
        command: command.to_owned(),
        source,
    }
}

/// Builds a process wait failure.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `source` - I/O error reported while waiting for the process.
///
/// # Returns
///
/// A command error preserving the command text and source error.
pub(crate) fn wait_failed(command: &str, source: io::Error) -> CommandError {
    CommandError::WaitFailed {
        command: command.to_owned(),
        source,
    }
}

/// Builds a timed-out process kill failure.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `timeout` - Timeout that had been exceeded.
/// * `source` - I/O error reported while killing the process.
///
/// # Returns
///
/// A command error preserving timeout and kill-failure context.
pub(crate) fn kill_failed(command: String, timeout: Duration, source: io::Error) -> CommandError {
    CommandError::KillFailed {
        command,
        timeout,
        source,
    }
}

/// Collects reader-thread results into a command output value.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `status` - Process exit status.
/// * `elapsed` - Observed command duration.
/// * `lossy_output` - Whether output text accessors should replace invalid
///   UTF-8 bytes.
/// * `stdout_reader` - Reader thread for stdout.
/// * `stderr_reader` - Reader thread for stderr.
/// * `stdin_writer` - Optional writer thread for configured stdin bytes.
///
/// # Returns
///
/// Command output containing both captured streams.
///
/// # Errors
///
/// Returns [`CommandError`] if stream collection or stdin writing fails. All
/// helper threads are joined before an error is returned.
pub(crate) fn collect_output(
    command: &str,
    status: ExitStatus,
    elapsed: Duration,
    lossy_output: bool,
    stdout_reader: OutputReader,
    stderr_reader: OutputReader,
    stdin_writer: StdinWriter,
) -> Result<CommandOutput, CommandError> {
    #[cfg(coverage)]
    crate::coverage_support::record_collect_output(command);

    #[cfg(coverage)]
    let forced_error =
        crate::coverage_support::forced_collect_output_error(command).map(|stream| {
            CommandError::ReadOutputFailed {
                command: command.to_owned(),
                stream,
                source: io::Error::other("forced output collection failure"),
            }
        });
    #[cfg(not(coverage))]
    let forced_error = None;

    let stdout_result = join_output_reader(command, OutputStream::Stdout, stdout_reader);
    let stderr_result = join_output_reader(command, OutputStream::Stderr, stderr_reader);
    let stdin_result = join_stdin_writer(command, stdin_writer);

    match (stdout_result, stderr_result, stdin_result, forced_error) {
        (Ok(stdout), Ok(stderr), Ok(()), None) => Ok(CommandOutput::new(
            status,
            stdout.bytes,
            stderr.bytes,
            stdout.truncated,
            stderr.truncated,
            elapsed,
            lossy_output,
        )),
        (Err(error), _, _, _)
        | (_, Err(error), _, _)
        | (_, _, Err(error), _)
        | (_, _, _, Some(error)) => Err(error),
    }
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
/// Captured bytes and truncation metadata for the requested stream.
///
/// # Errors
///
/// Returns [`CommandError`] when the reader reports I/O failure, tee-file write
/// failure, or panics.
pub(crate) fn join_output_reader(
    command: &str,
    stream: OutputStream,
    reader: OutputReader,
) -> Result<CapturedOutput, CommandError> {
    match reader.join() {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(OutputCaptureError::Read(source))) => Err(CommandError::ReadOutputFailed {
            command: command.to_owned(),
            stream,
            source,
        }),
        Ok(Err(OutputCaptureError::Write { path, source })) => {
            Err(CommandError::WriteOutputFailed {
                command: command.to_owned(),
                stream,
                path,
                source,
            })
        }
        Err(_) => Err(CommandError::ReadOutputFailed {
            command: command.to_owned(),
            stream,
            source: io::Error::other("output reader thread panicked"),
        }),
    }
}

/// Joins the stdin writer and maps failures to command errors.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `writer` - Optional stdin writer thread.
///
/// # Errors
///
/// Returns [`CommandError::WriteInputFailed`] when writing stdin fails or the
/// writer thread panics. A broken pipe is ignored because it only means the
/// child closed stdin before consuming all configured bytes; the process exit
/// status remains the authoritative command result.
pub(crate) fn join_stdin_writer(command: &str, writer: StdinWriter) -> Result<(), CommandError> {
    match writer {
        Some(writer) => match writer.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) if source.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Ok(Err(source)) => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source,
            }),
            Err(_) => Err(CommandError::WriteInputFailed {
                command: command.to_owned(),
                source: io::Error::other("stdin writer thread panicked"),
            }),
        },
        None => Ok(()),
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
pub(crate) fn output_pipe_error(command: &str, stream: OutputStream) -> CommandError {
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
pub(crate) fn next_sleep(timeout: Option<Duration>, elapsed: Duration) -> Duration {
    if let Some(timeout) = timeout
        && let Some(remaining) = timeout.checked_sub(elapsed)
    {
        return remaining.min(WAIT_POLL_INTERVAL);
    }
    WAIT_POLL_INTERVAL
}
