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
    time::{
        Duration,
        Instant,
    },
};

#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{
    ChildWrapper,
    CommandWrap,
};

use crate::command::CommandStdin;
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
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

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

        let mut child = match spawn_child(process_command, self.timeout.is_some()) {
            Ok(child) => child,
            Err(source) => return Err(spawn_failed(&command_text, source)),
        };

        let stdin_writer = write_stdin_bytes(&command_text, child.as_mut(), stdin_bytes)?;

        let stdout = match child.stdout().take() {
            Some(stdout) => stdout,
            None => return Err(output_pipe_error(&command_text, OutputStream::Stdout)),
        };
        let stderr = match child.stderr().take() {
            Some(stderr) => stderr,
            None => return Err(output_pipe_error(&command_text, OutputStream::Stderr)),
        };
        let stdout_reader = read_output_stream(
            stdout,
            OutputCaptureOptions::new(self.max_stdout_bytes, stdout_file, self.stdout_file.clone()),
        );
        let stderr_reader = read_output_stream(
            stderr,
            OutputCaptureOptions::new(self.max_stderr_bytes, stderr_file, self.stderr_file.clone()),
        );
        let command_io = CommandIo::new(stdout_reader, stderr_reader, stdin_writer);

        let start = Instant::now();
        let exit_status = loop {
            let maybe_status = match child.try_wait() {
                Ok(status) => status,
                Err(source) => {
                    let error = wait_failed(&command_text, source);
                    return Err(clean_up_after_wait_error(
                        &command_text,
                        child.as_mut(),
                        start.elapsed(),
                        self.lossy_output,
                        command_io,
                        error,
                    ));
                }
            };
            if let Some(status) = maybe_status {
                break status;
            }
            if let Some(timeout) = self.timeout
                && start.elapsed() >= timeout
            {
                if let Err(source) = child.start_kill() {
                    let error = kill_failed(command_text.clone(), timeout, source);
                    return Err(collect_if_child_exited(
                        &command_text,
                        child.as_mut(),
                        start.elapsed(),
                        self.lossy_output,
                        command_io,
                        error,
                    ));
                }
                let exit_status = match child.wait() {
                    Ok(status) => status,
                    Err(source) => {
                        let error = wait_failed(&command_text, source);
                        return Err(collect_if_child_exited(
                            &command_text,
                            child.as_mut(),
                            start.elapsed(),
                            self.lossy_output,
                            command_io,
                            error,
                        ));
                    }
                };
                let output = command_io.collect(
                    &command_text,
                    exit_status,
                    start.elapsed(),
                    self.lossy_output,
                )?;
                return Err(CommandError::TimedOut {
                    command: command_text,
                    timeout,
                    output: Box::new(output),
                });
            }
            thread::sleep(next_sleep(self.timeout, start.elapsed()));
        };

        let output = command_io.collect(
            &command_text,
            exit_status,
            start.elapsed(),
            self.lossy_output,
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

/// Output reader thread result type.
type OutputReader = thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>;

/// Stdin writer thread result type.
type StdinWriter = Option<thread::JoinHandle<io::Result<()>>>;

/// Output and stdin helper threads for one running command.
struct CommandIo {
    /// Reader thread draining stdout.
    stdout_reader: OutputReader,
    /// Reader thread draining stderr.
    stderr_reader: OutputReader,
    /// Optional writer thread feeding stdin.
    stdin_writer: StdinWriter,
}

impl CommandIo {
    /// Creates a command I/O helper bundle.
    ///
    /// # Parameters
    ///
    /// * `stdout_reader` - Reader thread draining stdout.
    /// * `stderr_reader` - Reader thread draining stderr.
    /// * `stdin_writer` - Optional writer thread feeding stdin.
    ///
    /// # Returns
    ///
    /// I/O helper bundle consumed when output is collected or drained.
    fn new(
        stdout_reader: OutputReader,
        stderr_reader: OutputReader,
        stdin_writer: StdinWriter,
    ) -> Self {
        Self {
            stdout_reader,
            stderr_reader,
            stdin_writer,
        }
    }

    /// Collects output from all helper threads.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text for diagnostics.
    /// * `status` - Process exit status.
    /// * `elapsed` - Observed command duration.
    /// * `lossy_output` - Whether text accessors should replace invalid UTF-8.
    ///
    /// # Returns
    ///
    /// Captured command output.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] if stream collection or stdin writing fails.
    fn collect(
        self,
        command: &str,
        status: ExitStatus,
        elapsed: Duration,
        lossy_output: bool,
    ) -> Result<CommandOutput, CommandError> {
        collect_output(
            command,
            status,
            elapsed,
            lossy_output,
            self.stdout_reader,
            self.stderr_reader,
            self.stdin_writer,
        )
    }
}

/// Captured output bytes plus truncation metadata.
#[derive(Debug)]
struct CapturedOutput {
    /// Bytes retained in memory.
    bytes: Vec<u8>,
    /// Whether emitted bytes exceeded the configured retention limit.
    truncated: bool,
}

/// Output capture options moved into a reader thread.
struct OutputCaptureOptions {
    /// Maximum bytes retained in memory.
    max_bytes: Option<usize>,
    /// Optional writer receiving a streaming copy.
    tee: Option<OutputTee>,
}

/// Streaming destination for captured output.
struct OutputTee {
    /// Writer receiving all emitted bytes.
    writer: Box<dyn Write + Send>,
    /// Path used for diagnostics if writes fail.
    path: PathBuf,
}

impl OutputCaptureOptions {
    /// Creates output capture options.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Optional in-memory retention limit.
    /// * `file` - Optional file receiving all emitted bytes.
    /// * `file_path` - File path used in write-failure diagnostics.
    ///
    /// # Returns
    ///
    /// Capture options moved into the output reader thread.
    fn new(max_bytes: Option<usize>, file: Option<File>, file_path: Option<PathBuf>) -> Self {
        let tee = file.map(|file| OutputTee {
            writer: Box::new(file),
            path: file_path.unwrap_or_default(),
        });
        Self { max_bytes, tee }
    }

    /// Creates output capture options from an arbitrary writer.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Optional in-memory retention limit.
    /// * `writer` - Writer receiving all emitted bytes.
    /// * `path` - Diagnostic path reported when writes fail.
    ///
    /// # Returns
    ///
    /// Capture options moved into the output reader thread.
    #[cfg(coverage)]
    fn new_writer(max_bytes: Option<usize>, writer: Box<dyn Write + Send>, path: PathBuf) -> Self {
        Self {
            max_bytes,
            tee: Some(OutputTee { writer, path }),
        }
    }
}

/// Error reported by an output reader thread.
#[derive(Debug)]
enum OutputCaptureError {
    /// Reading from the child pipe failed.
    Read(io::Error),
    /// Writing to a tee file failed.
    Write {
        /// Tee file path.
        path: PathBuf,
        /// I/O error reported by the writer.
        source: io::Error,
    },
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
) -> io::Result<Box<dyn ChildWrapper>> {
    #[cfg(coverage)]
    if coverage_support::fake_children_enabled()
        && let Some(child) = coverage_support::fake_child_for(process_command.get_program())
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
fn write_stdin_bytes(
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
fn read_output_stream<R>(reader: R, options: OutputCaptureOptions) -> OutputReader
where
    R: Read + Send + 'static,
{
    thread::spawn(move || read_output(reader, options))
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
fn read_output<R>(
    mut reader: R,
    mut options: OutputCaptureOptions,
) -> Result<CapturedOutput, OutputCaptureError>
where
    R: Read,
{
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
fn spawn_failed(command: &str, source: io::Error) -> CommandError {
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
fn wait_failed(command: &str, source: io::Error) -> CommandError {
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
fn kill_failed(command: String, timeout: Duration, source: io::Error) -> CommandError {
    CommandError::KillFailed {
        command,
        timeout,
        source,
    }
}

/// Attempts to terminate a child after a wait error and drain its I/O helpers.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `child` - Child process wrapper to clean up.
/// * `elapsed` - Elapsed command duration.
/// * `lossy_output` - Whether text accessors should replace invalid UTF-8.
/// * `command_io` - Output and stdin helper threads to collect when safe.
/// * `error` - Original error to return after cleanup.
///
/// # Returns
///
/// The original error after best-effort cleanup.
fn clean_up_after_wait_error(
    command: &str,
    child: &mut dyn ChildWrapper,
    elapsed: Duration,
    lossy_output: bool,
    command_io: CommandIo,
    error: CommandError,
) -> CommandError {
    if child.start_kill().is_ok()
        && let Ok(status) = child.wait()
    {
        let _ = command_io.collect(command, status, elapsed, lossy_output);
        return error;
    }
    collect_if_child_exited(command, child, elapsed, lossy_output, command_io, error)
}

/// Drains I/O helpers if the child is already known to have exited.
///
/// # Parameters
///
/// * `command` - Human-readable command text for diagnostics.
/// * `child` - Child process wrapper to inspect.
/// * `elapsed` - Elapsed command duration.
/// * `lossy_output` - Whether text accessors should replace invalid UTF-8.
/// * `command_io` - Output and stdin helper threads to collect when safe.
/// * `error` - Original error to return after cleanup.
///
/// # Returns
///
/// The original error. Output collection failures during cleanup are ignored so
/// the primary process-control failure remains visible.
fn collect_if_child_exited(
    command: &str,
    child: &mut dyn ChildWrapper,
    elapsed: Duration,
    lossy_output: bool,
    command_io: CommandIo,
    error: CommandError,
) -> CommandError {
    if let Ok(Some(status)) = child.try_wait() {
        let _ = command_io.collect(command, status, elapsed, lossy_output);
    }
    error
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
fn collect_output(
    command: &str,
    status: ExitStatus,
    elapsed: Duration,
    lossy_output: bool,
    stdout_reader: OutputReader,
    stderr_reader: OutputReader,
    stdin_writer: StdinWriter,
) -> Result<CommandOutput, CommandError> {
    #[cfg(coverage)]
    coverage_support::record_collect_output(command);

    #[cfg(coverage)]
    let forced_error = coverage_support::forced_collect_output_error(command).map(|stream| {
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
fn join_output_reader(
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
fn join_stdin_writer(command: &str, writer: StdinWriter) -> Result<(), CommandError> {
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

/// Coverage-only hooks for exercising defensive process-runner branches.
#[cfg(coverage)]
#[doc(hidden)]
pub mod coverage_support {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::{
        cell::Cell,
        cell::RefCell,
        ffi::OsStr,
        io::{
            self,
            Read,
            Write,
        },
        panic,
        path::PathBuf,
        process::{
            ChildStderr,
            ChildStdin,
            ChildStdout,
            Command as SyntheticCommand,
            ExitStatus,
            Stdio,
        },
        thread,
        time::Duration,
    };

    use process_wrap::std::ChildWrapper;

    use super::{
        CapturedOutput,
        OutputCaptureError,
        OutputCaptureOptions,
        WAIT_POLL_INTERVAL,
        collect_output,
        join_output_reader,
        join_stdin_writer,
        kill_failed,
        next_sleep,
        output_pipe_error,
        read_output,
        spawn_failed,
        wait_failed,
        write_stdin_bytes,
    };
    use crate::OutputStream;

    thread_local! {
        /// Whether synthetic children are enabled on this test thread.
        static FAKE_CHILDREN_ENABLED: Cell<bool> = const { Cell::new(false) };
        /// Commands whose output collection path has been reached.
        static COLLECT_OUTPUT_COMMANDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// Guard restoring synthetic-child state when dropped.
    struct FakeChildGuard {
        /// Previously configured state for this thread.
        previous: bool,
    }

    impl Drop for FakeChildGuard {
        /// Restores the previous synthetic-child state.
        fn drop(&mut self) {
            FAKE_CHILDREN_ENABLED.set(self.previous);
        }
    }

    /// Runs an operation with coverage-only synthetic children enabled.
    ///
    /// # Parameters
    ///
    /// * `operation` - Operation that may run magic coverage-only command
    ///   names.
    ///
    /// # Returns
    ///
    /// The value returned by `operation`.
    pub fn with_fake_children_enabled<T>(operation: impl FnOnce() -> T) -> T {
        let _guard = enable_fake_children();
        operation()
    }

    /// Returns whether coverage-only synthetic children are enabled.
    ///
    /// # Returns
    ///
    /// `true` only within [`with_fake_children_enabled`] on the current thread.
    pub(super) fn fake_children_enabled() -> bool {
        FAKE_CHILDREN_ENABLED.get()
    }

    /// Records that output collection was reached for a command.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text passed to output collection.
    pub(super) fn record_collect_output(command: &str) {
        COLLECT_OUTPUT_COMMANDS.with_borrow_mut(|commands| commands.push(command.to_owned()));
    }

    /// Takes and clears recorded output-collection commands.
    ///
    /// # Returns
    ///
    /// Recorded command texts since the previous call on this thread.
    pub fn take_collect_output_commands() -> Vec<String> {
        COLLECT_OUTPUT_COMMANDS.take()
    }

    /// Enables coverage-only synthetic children for the current thread.
    ///
    /// # Returns
    ///
    /// Guard restoring the previous state when dropped.
    fn enable_fake_children() -> FakeChildGuard {
        let previous = fake_children_enabled();
        FAKE_CHILDREN_ENABLED.set(true);
        FakeChildGuard { previous }
    }

    /// Exercises internal error helpers that cannot be reached reliably through
    /// real OS process execution.
    ///
    /// # Returns
    ///
    /// Diagnostic strings built from each exercised error path.
    pub fn exercise_defensive_paths() -> Vec<String> {
        let mut diagnostics = vec![
            spawn_failed("spawn", io::Error::other("spawn failed")).to_string(),
            wait_failed("wait", io::Error::other("wait failed")).to_string(),
            kill_failed(
                "kill".to_owned(),
                Duration::from_millis(1),
                io::Error::other("kill failed"),
            )
            .to_string(),
            output_pipe_error("pipe", OutputStream::Stdout).to_string(),
            output_pipe_error("pipe", OutputStream::Stderr).to_string(),
        ];

        let read_error = read_output(FailingReader, OutputCaptureOptions::new(None, None, None))
            .expect_err("failing reader should report read error");
        if let OutputCaptureError::Read(source) = read_error {
            diagnostics.push(source.to_string());
        }

        let write_error = read_output(
            io::Cursor::new(b"write".to_vec()),
            OutputCaptureOptions::new_writer(
                None,
                Box::new(FailingWrite),
                PathBuf::from("stdout.txt"),
            ),
        )
        .expect_err("failing writer should report write error");
        if let OutputCaptureError::Write { path, source } = write_error {
            diagnostics.push(path.display().to_string());
            diagnostics.push(source.to_string());
        }

        let flush_error = read_output(
            io::Cursor::new(b"flush".to_vec()),
            OutputCaptureOptions::new_writer(
                None,
                Box::new(FailingFlush),
                PathBuf::from("stderr.txt"),
            ),
        )
        .expect_err("failing flush should report write error");
        if let OutputCaptureError::Write { path, source } = flush_error {
            diagnostics.push(path.display().to_string());
            diagnostics.push(source.to_string());
        }

        let mut no_stdin_child = NoStdinChild::default();
        diagnostics.push(format!("{}", no_stdin_child.id()));
        diagnostics.push(format!("{}", no_stdin_child.inner().id()));
        diagnostics.push(format!("{}", no_stdin_child.inner_mut().id()));
        diagnostics.push(format!("{}", no_stdin_child.stdout().is_none()));
        diagnostics.push(format!("{}", no_stdin_child.stderr().is_none()));
        diagnostics.push(format!(
            "{}",
            no_stdin_child
                .try_wait()
                .expect("synthetic try_wait should succeed")
                .is_some(),
        ));
        no_stdin_child
            .start_kill()
            .expect("synthetic kill should succeed");
        diagnostics.push(format!(
            "{}",
            no_stdin_child
                .wait()
                .expect("synthetic wait should succeed")
                .success(),
        ));
        diagnostics.push(
            write_stdin_bytes(
                "missing-stdin",
                &mut no_stdin_child,
                Some(b"input".to_vec()),
            )
            .expect_err("missing child stdin should be reported")
            .to_string(),
        );
        let boxed_child = Box::new(NoStdinChild::default()).into_inner();
        diagnostics.push(format!("{}", boxed_child.id()));

        let mut failing_write = FailingWrite;
        failing_write
            .flush()
            .expect("synthetic flush should succeed");

        let failed_stdout = reader_read_error("collect stdout failed");
        let empty_stderr = reader_ok(Vec::new());
        diagnostics.push(
            collect_output(
                "collect-stdout",
                success_status(),
                Duration::ZERO,
                false,
                failed_stdout,
                empty_stderr,
                None,
            )
            .expect_err("stdout collection error should be mapped")
            .to_string(),
        );

        let empty_stdout = reader_ok(Vec::new());
        let failed_stderr = reader_read_error("collect stderr failed");
        diagnostics.push(
            collect_output(
                "collect-stderr",
                success_status(),
                Duration::ZERO,
                false,
                empty_stdout,
                failed_stderr,
                None,
            )
            .expect_err("stderr collection error should be mapped")
            .to_string(),
        );

        let empty_stdout = reader_ok(Vec::new());
        let empty_stderr = reader_ok(Vec::new());
        diagnostics.push(
            collect_output(
                "collect-stdin",
                success_status(),
                Duration::ZERO,
                false,
                empty_stdout,
                empty_stderr,
                Some(thread::spawn(|| {
                    Err(io::Error::other("collect stdin failed"))
                })),
            )
            .expect_err("stdin collection error should be mapped")
            .to_string(),
        );

        let reader_error = reader_read_error("reader failed");
        diagnostics.push(
            join_output_reader("reader", OutputStream::Stdout, reader_error)
                .expect_err("reader error should be mapped")
                .to_string(),
        );

        let writer_error = reader_write_error("reader write failed");
        diagnostics.push(
            join_output_reader("writer", OutputStream::Stdout, writer_error)
                .expect_err("writer error should be mapped")
                .to_string(),
        );

        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let panicked_reader = thread::spawn(|| -> Result<CapturedOutput, OutputCaptureError> {
            panic!("output reader panic");
        });
        let panic_error = join_output_reader("panic", OutputStream::Stderr, panicked_reader)
            .expect_err("reader panic should be mapped")
            .to_string();
        panic::set_hook(previous_hook);
        diagnostics.push(panic_error);

        diagnostics.push(
            join_stdin_writer(
                "stdin-write",
                Some(thread::spawn(|| {
                    Err(io::Error::other("stdin write failed"))
                })),
            )
            .expect_err("stdin writer error should be mapped")
            .to_string(),
        );

        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let stdin_panicked = thread::spawn(|| -> io::Result<()> {
            panic!("stdin writer panic");
        });
        let stdin_panic_error = join_stdin_writer("stdin-panic", Some(stdin_panicked))
            .expect_err("stdin writer panic should be mapped")
            .to_string();
        panic::set_hook(previous_hook);
        diagnostics.push(stdin_panic_error);

        diagnostics.push(format!("{:?}", next_sleep(None, Duration::ZERO)));
        diagnostics.push(format!(
            "{:?}",
            next_sleep(Some(Duration::from_millis(1)), Duration::from_millis(2)),
        ));
        diagnostics.push(format!(
            "{:?}",
            next_sleep(Some(Duration::from_secs(1)), Duration::ZERO),
        ));
        diagnostics.push(format!("{WAIT_POLL_INTERVAL:?}"));
        diagnostics.push(format!(
            "{}",
            fake_child_for(OsStr::new("__qubit_command_normal_child__")).is_none(),
        ));
        diagnostics.push(format!("{}", fake_children_enabled()));
        with_fake_children_enabled(|| {
            diagnostics.push(format!("{}", fake_children_enabled()));
        });
        diagnostics.push(format!("{}", fake_children_enabled()));
        diagnostics
    }

    /// Creates a successful exit status for coverage-only helper calls.
    ///
    /// # Returns
    ///
    /// Platform-specific successful process exit status.
    fn success_status() -> ExitStatus {
        ExitStatus::from_raw(0)
    }

    /// Creates an output reader that succeeds with the supplied bytes.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Bytes returned by the synthetic reader.
    ///
    /// # Returns
    ///
    /// Output reader join handle.
    fn reader_ok(bytes: Vec<u8>) -> super::OutputReader {
        thread::spawn(move || {
            Ok(CapturedOutput {
                bytes,
                truncated: false,
            })
        })
    }

    /// Creates an output reader that fails with a read error.
    ///
    /// # Parameters
    ///
    /// * `message` - Error message used by the synthetic reader.
    ///
    /// # Returns
    ///
    /// Output reader join handle.
    fn reader_read_error(message: &'static str) -> super::OutputReader {
        thread::spawn(move || Err(OutputCaptureError::Read(io::Error::other(message))))
    }

    /// Creates an output reader that fails with a tee-write error.
    ///
    /// # Parameters
    ///
    /// * `message` - Error message used by the synthetic writer.
    ///
    /// # Returns
    ///
    /// Output reader join handle.
    fn reader_write_error(message: &'static str) -> super::OutputReader {
        thread::spawn(move || {
            Err(OutputCaptureError::Write {
                path: PathBuf::from("reader-output.txt"),
                source: io::Error::other(message),
            })
        })
    }

    /// Creates a synthetic child for coverage-only run-loop branches.
    ///
    /// # Parameters
    ///
    /// * `program` - Program name passed to the process command.
    ///
    /// # Returns
    ///
    /// A synthetic child for known coverage-only program names, otherwise
    /// `None` so normal process spawning proceeds.
    pub(super) fn fake_child_for(program: &OsStr) -> Option<Box<dyn ChildWrapper>> {
        let child = match program.to_string_lossy().as_ref() {
            "__qubit_command_missing_stdout__" => NoStdinChild::default(),
            "__qubit_command_missing_stderr__" => child_with_stdout_only(),
            "__qubit_command_try_wait_error__" => child_with_try_wait_error(),
            "__qubit_command_try_wait_error_kill_cleanup__" => {
                child_with_try_wait_error_kill_cleanup()
            }
            "__qubit_command_kill_error__" => child_with_kill_error(),
            "__qubit_command_wait_after_kill_error__" => child_with_wait_after_kill_error(),
            "__qubit_command_collect_output_error__" => child_with_output_pipes(),
            "__qubit_command_timeout_collect_output_error__" => child_pending_with_output_pipes(),
            _ => return None,
        };
        Some(Box::new(child))
    }

    /// Checks whether output collection should fail for a synthetic command.
    ///
    /// # Parameters
    ///
    /// * `command` - Human-readable command text built by the runner.
    ///
    /// # Returns
    ///
    /// The stream to report as failed for known synthetic command names,
    /// otherwise `None`.
    pub(super) fn forced_collect_output_error(command: &str) -> Option<OutputStream> {
        if command.contains("__qubit_command_collect_output_error__")
            || command.contains("__qubit_command_timeout_collect_output_error__")
        {
            Some(OutputStream::Stdout)
        } else {
            None
        }
    }

    /// Creates a synthetic child with both output pipes available.
    ///
    /// # Returns
    ///
    /// Child wrapper state used to reach output collection through the normal
    /// process completion branch.
    fn child_with_output_pipes() -> NoStdinChild {
        NoStdinChild {
            stdout: Some(empty_stdout()),
            stderr: Some(empty_stderr()),
            ..NoStdinChild::default()
        }
    }

    /// Creates a pending synthetic child with both output pipes available.
    ///
    /// # Returns
    ///
    /// Child wrapper state used to reach output collection through the timeout
    /// branch.
    fn child_pending_with_output_pipes() -> NoStdinChild {
        NoStdinChild {
            stdout: Some(empty_stdout()),
            stderr: Some(empty_stderr()),
            pending: true,
            ..NoStdinChild::default()
        }
    }

    /// Creates a synthetic child with only stdout available.
    ///
    /// # Returns
    ///
    /// Child wrapper state used to exercise missing-stderr handling.
    fn child_with_stdout_only() -> NoStdinChild {
        NoStdinChild {
            stdout: Some(empty_stdout()),
            ..NoStdinChild::default()
        }
    }

    /// Creates a synthetic child whose try-wait operation fails.
    ///
    /// # Returns
    ///
    /// Child wrapper state used to exercise wait-error handling.
    fn child_with_try_wait_error() -> NoStdinChild {
        NoStdinChild {
            stdout: Some(empty_stdout()),
            stderr: Some(empty_stderr()),
            try_wait_error: Some("try wait failed"),
            ..NoStdinChild::default()
        }
    }

    /// Creates a synthetic child whose wait-error cleanup uses the fallback.
    ///
    /// # Returns
    ///
    /// Child wrapper state used to exercise cleanup after try-wait and kill
    /// errors.
    fn child_with_try_wait_error_kill_cleanup() -> NoStdinChild {
        NoStdinChild {
            stdout: Some(empty_stdout()),
            stderr: Some(empty_stderr()),
            try_wait_error: Some("try wait failed"),
            clear_try_wait_error_after_first: true,
            pending: true,
            kill_error: Some("cleanup kill failed"),
            exited_after_kill_attempt: true,
            ..NoStdinChild::default()
        }
    }

    /// Creates a synthetic child whose kill operation fails.
    ///
    /// # Returns
    ///
    /// Child wrapper state used to exercise kill-error handling.
    fn child_with_kill_error() -> NoStdinChild {
        NoStdinChild {
            stdout: Some(empty_stdout()),
            stderr: Some(empty_stderr()),
            pending: true,
            kill_error: Some("kill failed"),
            exited_after_kill_attempt: true,
            ..NoStdinChild::default()
        }
    }

    /// Creates a synthetic child whose wait after kill fails.
    ///
    /// # Returns
    ///
    /// Child wrapper state used to exercise post-timeout wait-error handling.
    fn child_with_wait_after_kill_error() -> NoStdinChild {
        NoStdinChild {
            stdout: Some(empty_stdout()),
            stderr: Some(empty_stderr()),
            pending: true,
            wait_error: Some("wait after kill failed"),
            exited_after_kill_attempt: true,
            ..NoStdinChild::default()
        }
    }

    /// Creates an empty stdout pipe handle.
    ///
    /// # Returns
    ///
    /// A child stdout handle that is already at EOF.
    fn empty_stdout() -> ChildStdout {
        let mut child = empty_process_command()
            .stdout(Stdio::piped())
            .spawn()
            .expect("synthetic stdout child should spawn");
        let stdout = child
            .stdout
            .take()
            .expect("synthetic stdout should be piped");
        child.wait().expect("synthetic stdout child should finish");
        stdout
    }

    /// Creates an empty stderr pipe handle.
    ///
    /// # Returns
    ///
    /// A child stderr handle that is already at EOF.
    fn empty_stderr() -> ChildStderr {
        let mut child = empty_process_command()
            .stderr(Stdio::piped())
            .spawn()
            .expect("synthetic stderr child should spawn");
        let stderr = child
            .stderr
            .take()
            .expect("synthetic stderr should be piped");
        child.wait().expect("synthetic stderr child should finish");
        stderr
    }

    /// Creates a platform shell command that exits without output.
    ///
    /// # Returns
    ///
    /// A process command used only to obtain pipe handles for synthetic
    /// children.
    fn empty_process_command() -> SyntheticCommand {
        #[cfg(not(windows))]
        {
            let mut command = SyntheticCommand::new("sh");
            command.arg("-c").arg(":");
            command
        }
        #[cfg(windows)]
        {
            let mut command = SyntheticCommand::new("cmd");
            command.arg("/C").arg("exit /B 0");
            command
        }
    }

    /// Child wrapper without a stdin pipe.
    #[derive(Debug, Default)]
    struct NoStdinChild {
        /// Synthetic stdin pipe.
        stdin: Option<ChildStdin>,
        /// Synthetic stdout pipe.
        stdout: Option<ChildStdout>,
        /// Synthetic stderr pipe.
        stderr: Option<ChildStderr>,
        /// Error returned by synthetic try-wait.
        try_wait_error: Option<&'static str>,
        /// Whether the synthetic try-wait error is reported only once.
        clear_try_wait_error_after_first: bool,
        /// Whether synthetic try-wait reports a still-running child.
        pending: bool,
        /// Whether try-wait reports exit after a kill attempt.
        exited_after_kill_attempt: bool,
        /// Whether kill has been attempted.
        kill_attempted: bool,
        /// Error returned by synthetic kill.
        kill_error: Option<&'static str>,
        /// Error returned by synthetic wait.
        wait_error: Option<&'static str>,
    }

    impl ChildWrapper for NoStdinChild {
        /// Returns this synthetic child as the innermost wrapper.
        fn inner(&self) -> &dyn ChildWrapper {
            self
        }

        /// Returns this synthetic child as the innermost mutable wrapper.
        fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
            self
        }

        /// Consumes and returns this synthetic child.
        fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
            self
        }

        /// Returns the absent synthetic stdin pipe.
        fn stdin(&mut self) -> &mut Option<ChildStdin> {
            &mut self.stdin
        }

        /// Returns the absent synthetic stdout pipe.
        fn stdout(&mut self) -> &mut Option<ChildStdout> {
            &mut self.stdout
        }

        /// Returns the absent synthetic stderr pipe.
        fn stderr(&mut self) -> &mut Option<ChildStderr> {
            &mut self.stderr
        }

        /// Returns a dummy process identifier.
        fn id(&self) -> u32 {
            0
        }

        /// Reports that the synthetic child has already exited successfully.
        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            if let Some(message) = self.try_wait_error {
                if self.clear_try_wait_error_after_first {
                    self.try_wait_error = None;
                }
                Err(io::Error::other(message))
            } else if self.pending && !(self.kill_attempted && self.exited_after_kill_attempt) {
                Ok(None)
            } else {
                Ok(Some(success_status()))
            }
        }

        /// Reports a successful synthetic process exit.
        fn wait(&mut self) -> io::Result<ExitStatus> {
            if let Some(message) = self.wait_error {
                Err(io::Error::other(message))
            } else {
                Ok(success_status())
            }
        }

        /// Reports a successful synthetic kill.
        fn start_kill(&mut self) -> io::Result<()> {
            self.kill_attempted = true;
            if let Some(message) = self.kill_error {
                Err(io::Error::other(message))
            } else {
                Ok(())
            }
        }
    }

    /// Reader that always fails when read.
    struct FailingReader;

    impl Read for FailingReader {
        /// Reports a synthetic read failure.
        ///
        /// # Parameters
        ///
        /// * `_buffer` - Destination buffer intentionally left untouched.
        ///
        /// # Returns
        ///
        /// Always returns an I/O error.
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read failed"))
        }
    }

    /// Writer that always fails when bytes are written.
    struct FailingWrite;

    impl Write for FailingWrite {
        /// Reports a synthetic write failure.
        ///
        /// # Parameters
        ///
        /// * `_buffer` - Bytes intentionally not written.
        ///
        /// # Returns
        ///
        /// Always returns an I/O error.
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("write failed"))
        }

        /// Flushes the synthetic writer.
        ///
        /// # Returns
        ///
        /// Always succeeds because the write path is tested separately.
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Writer that accepts bytes but fails when flushed.
    struct FailingFlush;

    impl Write for FailingFlush {
        /// Pretends all bytes were written successfully.
        ///
        /// # Parameters
        ///
        /// * `buffer` - Bytes accepted by the synthetic writer.
        ///
        /// # Returns
        ///
        /// Number of bytes accepted.
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        /// Reports a synthetic flush failure.
        ///
        /// # Returns
        ///
        /// Always returns an I/O error.
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush failed"))
        }
    }
}
