// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fmt;

mod managed_command_runner_api;
use std::io;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub use managed_command_runner_api::ManagedCommandRunnerApi;
use qubit_clock::StdTimer;
use qubit_clock::Timer;
use qubit_redact::RedactionPolicy;
use qubit_redact::Redactor;

mod internal;
mod run_tree_policy;
use internal::committed_command::CommittedCommand;
use internal::error_mapping::output_pipe_error;
use internal::error_mapping::spawn_failed;
use internal::file_tee::FileTee;
use internal::finished_command::FinishedCommand;
use internal::output_capture_options::OutputCaptureOptions;
use internal::output_collector::read_output_stream;
use internal::output_reader::OutputReader;
use internal::prepared_command::PreparedCommand;
use internal::process_launcher::spawn_child;
use internal::running_command::RunningCommand;
use internal::running_command_resources::RunningCommandResources;
use internal::starting_command::StartingCommand;
use internal::stdin_pipe::write_stdin_session;
use internal::total_output_budget::TotalOutputBudget;
pub(crate) use run_tree_policy::RunTreePolicy;

use crate::Command;
use crate::CommandCancellation;
use crate::CommandError;
use crate::CommandErrorReason;
use crate::CommandOutput;
use crate::CommandRunOptions;
use crate::FileSinkPolicy;
use crate::OutputStream;
use crate::ProcessTreePolicy;
use crate::command_run_options_parts::CommandRunOptionsParts;
use crate::execution::contracts::PreparedFiles;

/// Marker used when a configured working directory is redacted from
/// diagnostics.
const REDACTED_PATH: &str = "<redacted path>";

/// Takes a prepared child output pipe and maps an absent pipe to a runner
/// error.
///
/// # Type Parameters
///
/// * `T` - Type carried by the output pipe.
///
/// # Parameters
///
/// * `command` - Redacted command text used in a missing-pipe error.
/// * `stream` - Output stream whose pipe is requested.
/// * `take` - Closure that removes the pipe from the child process.
fn take_output_pipe<T>(
    command: &str,
    stream: OutputStream,
    take: impl FnOnce() -> Option<T>,
) -> Result<T, CommandError> {
    take().ok_or_else(|| output_pipe_error(command, stream))
}

/// Starts one output reader and annotates thread-start failures with its
/// stream.
///
/// # Parameters
///
/// * `command` - Redacted command text used in thread-start errors.
/// * `stream` - Output stream assigned to the reader.
/// * `start` - Closure that starts the reader and returns its handle.
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

/// Validates owned file resources and rejects aliases between stdin and output.
///
/// # Parameters
///
/// * `command` - Redacted command text used in any validation error.
/// * `files` - Prepared handles for stdin, stdout, and stderr.
/// * `prepared` - Command configuration containing path-based tees.
///
/// # Returns
///
/// `Ok(())` when the resources can be used together.
///
/// # Errors
///
/// Returns a write-output error when resources conflict or cannot be cloned.
fn validate_prepared_files(
    command: &str,
    files: &PreparedFiles,
    prepared: &PreparedCommand,
) -> Result<(), CommandError> {
    if (files.stdout.is_some() || files.stderr.is_some())
        && (prepared.stdout_file_path.is_some() || prepared.stderr_file_path.is_some())
    {
        return Err(owned_file_error(
            command,
            OutputStream::Stdout,
            "<owned sink>",
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "owned sinks cannot be combined with path tees",
            ),
        ));
    }
    let stdin = files
        .stdin
        .as_ref()
        .map(|resource| resource.lock().unwrap_or_else(|e| e.into_inner()).try_clone())
        .transpose()
        .map_err(|source| owned_file_error(command, OutputStream::Stdout, "<owned stdin>", source))?;
    let stdout = files
        .stdout
        .as_ref()
        .map(|sink| sink.file.lock().unwrap_or_else(|e| e.into_inner()).try_clone())
        .transpose()
        .map_err(|source| owned_file_error(command, OutputStream::Stdout, "<owned stdout>", source))?;
    let stderr = files
        .stderr
        .as_ref()
        .map(|sink| sink.file.lock().unwrap_or_else(|e| e.into_inner()).try_clone())
        .transpose()
        .map_err(|source| owned_file_error(command, OutputStream::Stderr, "<owned stderr>", source))?;
    let handle = |file: &std::fs::File| same_file::Handle::from_file(file.try_clone()?);
    let stdin_handle = stdin
        .as_ref()
        .map(handle)
        .transpose()
        .map_err(|source| owned_file_error(command, OutputStream::Stdout, "<owned stdin>", source))?;
    let stdout_handle = stdout
        .as_ref()
        .map(handle)
        .transpose()
        .map_err(|source| owned_file_error(command, OutputStream::Stdout, "<owned stdout>", source))?;
    let stderr_handle = stderr
        .as_ref()
        .map(handle)
        .transpose()
        .map_err(|source| owned_file_error(command, OutputStream::Stderr, "<owned stderr>", source))?;
    if stdin_handle
        .as_ref()
        .zip(stdout_handle.as_ref())
        .is_some_and(|(a, b)| a == b)
        || stdin_handle
            .as_ref()
            .zip(stderr_handle.as_ref())
            .is_some_and(|(a, b)| a == b)
    {
        return Err(owned_file_error(
            command,
            OutputStream::Stdout,
            "<owned sink>",
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdin and output sink refer to the same file",
            ),
        ));
    }
    if stdout_handle
        .as_ref()
        .zip(stderr_handle.as_ref())
        .is_some_and(|(a, b)| a == b)
    {
        return Err(owned_file_error(
            command,
            OutputStream::Stderr,
            "<owned stderr>",
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdout and stderr sinks refer to the same file",
            ),
        ));
    }
    Ok(())
}

/// Installs prepared stdin and output sinks on the committed child command.
///
/// # Parameters
///
/// * `command` - Redacted command text used in I/O errors.
/// * `files` - Prepared file resources to install.
/// * `committed` - Spawn-ready command whose standard streams are configured.
///
/// # Returns
///
/// `Ok(())` after all configured resources are installed.
///
/// # Errors
///
/// Returns an error if a sink cannot be cloned or configured.
fn apply_prepared_files(
    command: &str,
    files: &PreparedFiles,
    committed: &mut CommittedCommand,
) -> Result<(), CommandError> {
    if let Some(stdin) = files.stdin.as_ref() {
        let file = stdin
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_clone()
            .map_err(|source| owned_file_error(command, OutputStream::Stdout, "<owned stdin>", source))?;
        committed.process_command.stdin(std::process::Stdio::from(file));
    }
    if let Some(sink) = files.stdout.as_ref() {
        committed.stdout_tee = Some(owned_sink(command, OutputStream::Stdout, sink)?);
    }
    if let Some(sink) = files.stderr.as_ref() {
        committed.stderr_tee = Some(owned_sink(command, OutputStream::Stderr, sink)?);
    }
    Ok(())
}

/// Creates a tee writer for an executor-owned output sink.
///
/// # Parameters
///
/// * `command` - Redacted command text used in any write error.
/// * `stream` - Output stream receiving the tee.
/// * `sink` - Shared file handle and append/truncate policy.
///
/// # Returns
///
/// A tee targeting the prepared file.
///
/// # Errors
///
/// Returns a write-output error if cloning or truncating the file fails.
fn owned_sink(
    command: &str,
    stream: OutputStream,
    sink: &crate::execution::contracts::PreparedSink,
) -> Result<FileTee, CommandError> {
    use std::io::Write;
    let mut file = sink
        .file
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .try_clone()
        .map_err(|source| owned_file_error(command, stream, "<owned sink>", source))?;
    if sink.policy == FileSinkPolicy::Truncate {
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .map_err(|source| owned_file_error(command, stream, "<owned sink>", source))?;
    }
    let _ = file.flush();
    Ok(FileTee::new(file, std::path::PathBuf::from("<owned sink>")))
}

/// Builds a stream-specific command error for an owned-file operation.
///
/// # Parameters
///
/// * `command` - Redacted command text associated with the failure.
/// * `stream` - Stream whose file operation failed.
/// * `path` - Safe diagnostic label for the file.
/// * `source` - Underlying operating-system error.
///
/// # Returns
///
/// A command error classified as a failed output write.
fn owned_file_error(command: &str, stream: OutputStream, path: &str, source: io::Error) -> CommandError {
    CommandError::from_reason(
        command,
        CommandErrorReason::WriteOutputFailed {
            stream,
            path: std::path::PathBuf::from(path),
            source,
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io;
    #[cfg(unix)]
    use std::io::Read;
    #[cfg(unix)]
    use std::io::Seek;
    #[cfg(unix)]
    use std::io::SeekFrom;
    #[cfg(unix)]
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;

    use qubit_clock::ManualMonotonicClock;
    use qubit_clock::MonotonicClock;
    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::ManagerOptions;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::OperationScope;
    use qubit_operation::TimerRegistration;
    use qubit_redact::RedactionPolicy;
    use qubit_redact::Sensitivity;

    use super::CommandRunner;
    use super::RunTreePolicy;
    #[cfg(unix)]
    use super::apply_prepared_files;
    use super::start_output_reader;
    use super::take_output_pipe;
    use super::validate_prepared_files;
    use crate::CommandCancellation;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;
    use crate::CommandRunOptions;
    use crate::FileSinkPolicy;
    use crate::OutputStream;
    use crate::command_runner::internal::prepared_command::PreparedCommand;
    use crate::execution::contracts::PreparedFiles;
    use crate::execution::contracts::PreparedSink;

    fn prepared_command(
        program: &str,
        stdout_path: Option<&std::path::Path>,
        stderr_path: Option<&std::path::Path>,
    ) -> PreparedCommand {
        PreparedCommand::prepare(
            crate::Command::new(program),
            &RedactionPolicy::default(),
            None,
            stdout_path,
            stderr_path,
            false,
        )
        .expect("test command should prepare")
    }

    struct FixedClock(MonotonicTime);

    impl ClockSource for FixedClock {
        fn now(&self) -> MonotonicTime {
            self.0
        }

        fn register_deadline(&self, _at: MonotonicTime, _waker: std::task::Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopTimerRegistration)
        }
    }

    struct NoopTimerRegistration;

    impl TimerRegistration for NoopTimerRegistration {
        fn cancel(self: Box<Self>) {}
    }

    #[test]
    fn test_expired_operation_context_stops_before_command_spawn() {
        let domain = ClockDomainId::new_unique();
        let context = OperationContext::new(ContextOptions {
            clock: Some(Arc::new(FixedClock(MonotonicTime::new(domain, 1)))),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 0))),
            budget: None,
        })
        .expect("clock and deadline should share a domain");

        let error = CommandRunner::without_timeout()
            .run_with_tree_policy(
                crate::Command::new("__expired_operation_must_not_spawn__"),
                CommandRunOptions::new(),
                RunTreePolicy {
                    operation_context: Some(context.clone()),
                    ..Default::default()
                },
            )
            .expect_err("an expired operation context should stop before spawning");

        assert_eq!(error.kind(), CommandErrorKind::CancelledBeforeStart);
        assert_eq!(context.stop_reason(), Some(qubit_operation::StopReason::Timeout));
    }

    #[test]
    fn test_runner_policy_builders_and_accessors_round_trip() {
        let timeout_runner = CommandRunner::new(Duration::from_millis(25));
        assert_eq!(timeout_runner.configured_timeout(), Some(Duration::from_millis(25)));
        assert_eq!(CommandRunner::without_timeout().configured_timeout(), None);

        let working_directory = std::env::temp_dir();
        let timer = Arc::new(qubit_clock::StdTimer::new());
        let redaction_policy = RedactionPolicy::default()
            .to_builder()
            .fields(|fields| {
                fields.raise("runner_token", Sensitivity::Secret);
            })
            .expect("test redaction field should be valid")
            .build()
            .expect("test redaction policy should be valid");
        let runner = CommandRunner::without_timeout()
            .working_directory(working_directory.clone())
            .success_exit_codes(&[0, 7])
            .disable_logging(true)
            .fail_on_output_truncation(true)
            .max_stdout_bytes(3)
            .max_stderr_bytes(5)
            .max_output_bytes(9)
            .bounded_output(11)
            .max_total_output_bytes(17)
            .timer(timer)
            .diagnostic_redaction_policy(redaction_policy.clone());

        assert_eq!(runner.configured_working_directory(), Some(working_directory.as_path()));
        assert_eq!(runner.configured_success_exit_codes(), &[0, 7]);
        assert!(runner.is_logging_disabled());
        assert!(runner.is_output_truncation_failure_enabled());
        assert_eq!(runner.configured_max_stdout_bytes(), Some(11));
        assert_eq!(runner.configured_max_stderr_bytes(), Some(11));
        assert_eq!(runner.configured_max_total_output_bytes(), Some(17));
        let _ = runner.configured_timer();
        assert_eq!(runner.configured_diagnostic_redaction_policy(), &redaction_policy);

        let unbounded = runner.unbounded_output().without_total_output_limit();
        assert_eq!(unbounded.configured_max_stdout_bytes(), None);
        assert_eq!(unbounded.configured_max_stderr_bytes(), None);
        assert!(!unbounded.is_output_truncation_failure_enabled());
        assert_eq!(unbounded.configured_max_total_output_bytes(), None);
        let second_working_directory = std::env::temp_dir();
        assert_eq!(
            CommandRunner::without_timeout()
                .working_directory(second_working_directory.clone())
                .configured_working_directory(),
            Some(second_working_directory.as_path())
        );
    }

    #[test]
    fn test_runner_policy_accessors_preserve_independent_limits_and_builder_order() {
        let clock = ManualMonotonicClock::new_shared();
        let timer_domain = clock.now().domain();
        let working_directory = std::env::temp_dir().join("runner-policy-accessor-test");
        let runner = CommandRunner::new(Duration::from_millis(125))
            .timer(Arc::new(clock.new_timer()))
            .working_directory(working_directory.clone())
            .success_exit_code(7)
            .success_exit_codes(&[3, 7])
            .max_stdout_bytes(2)
            .max_stderr_bytes(4)
            .fail_on_output_truncation(false)
            .max_total_output_bytes(19)
            .disable_logging(true)
            .bounded_output(6);

        assert_eq!(runner.configured_timeout(), Some(Duration::from_millis(125)));
        assert_eq!(runner.configured_timer().clock().now().domain(), timer_domain);
        assert_eq!(runner.configured_working_directory(), Some(working_directory.as_path()));
        assert_eq!(runner.configured_success_exit_codes(), &[3, 7]);
        assert!(runner.is_logging_disabled());
        assert!(runner.is_output_truncation_failure_enabled());
        assert_eq!(runner.configured_max_stdout_bytes(), Some(6));
        assert_eq!(runner.configured_max_stderr_bytes(), Some(6));
        assert_eq!(runner.configured_max_total_output_bytes(), Some(19));

        let without_memory_limits = runner.clone().unbounded_output();
        assert_eq!(without_memory_limits.configured_max_stdout_bytes(), None);
        assert_eq!(without_memory_limits.configured_max_stderr_bytes(), None);
        assert!(!without_memory_limits.is_output_truncation_failure_enabled());
        assert_eq!(without_memory_limits.configured_max_total_output_bytes(), Some(19));

        let without_total_budget = runner.without_total_output_limit();
        assert_eq!(without_total_budget.configured_max_stdout_bytes(), Some(6));
        assert_eq!(without_total_budget.configured_max_stderr_bytes(), Some(6));
        assert_eq!(without_total_budget.configured_max_total_output_bytes(), None);
        assert!(without_total_budget.is_output_truncation_failure_enabled());
    }

    #[test]
    fn test_runner_policy_method_items_round_trip_configuration() {
        let new: fn(Duration) -> CommandRunner = std::hint::black_box(CommandRunner::new);
        let with_directory: fn(CommandRunner, std::path::PathBuf) -> CommandRunner =
            std::hint::black_box(CommandRunner::working_directory);
        let with_success_code: fn(CommandRunner, i32) -> CommandRunner =
            std::hint::black_box(CommandRunner::success_exit_code);
        let with_success_codes: fn(CommandRunner, &[i32]) -> CommandRunner =
            std::hint::black_box(CommandRunner::success_exit_codes);
        let with_logging: fn(CommandRunner, bool) -> CommandRunner =
            std::hint::black_box(CommandRunner::disable_logging);
        let with_truncation: fn(CommandRunner, bool) -> CommandRunner =
            std::hint::black_box(CommandRunner::fail_on_output_truncation);
        let with_stdout_limit: fn(CommandRunner, usize) -> CommandRunner =
            std::hint::black_box(CommandRunner::max_stdout_bytes);
        let with_stderr_limit: fn(CommandRunner, usize) -> CommandRunner =
            std::hint::black_box(CommandRunner::max_stderr_bytes);
        let with_output_limit: fn(CommandRunner, usize) -> CommandRunner =
            std::hint::black_box(CommandRunner::max_output_bytes);
        let with_total_limit: fn(CommandRunner, u64) -> CommandRunner =
            std::hint::black_box(CommandRunner::max_total_output_bytes);
        let timeout: fn(&CommandRunner) -> Option<Duration> = std::hint::black_box(CommandRunner::configured_timeout);
        let directory: for<'a> fn(&'a CommandRunner) -> Option<&'a std::path::Path> =
            std::hint::black_box(CommandRunner::configured_working_directory);
        let success_codes: for<'a> fn(&'a CommandRunner) -> &'a [i32] =
            std::hint::black_box(CommandRunner::configured_success_exit_codes);

        let expected_directory = std::env::temp_dir();
        let runner = with_directory(new(Duration::from_secs(3)), expected_directory.clone());
        let runner = with_success_code(runner, 0);
        let runner = with_success_codes(runner, &[0, 7]);
        let runner = with_logging(runner, true);
        let runner = with_truncation(runner, false);
        let runner = with_stdout_limit(runner, 13);
        let runner = with_stderr_limit(runner, 17);
        let runner = with_output_limit(runner, 19);
        let runner = with_total_limit(runner, 23);

        assert_eq!(timeout(&runner), Some(Duration::from_secs(3)));
        assert_eq!(directory(&runner), Some(expected_directory.as_path()));
        assert_eq!(success_codes(&runner), &[0, 7]);
        assert!(runner.is_logging_disabled());
        assert!(!runner.is_output_truncation_failure_enabled());
        assert_eq!(runner.configured_max_stdout_bytes(), Some(19));
        assert_eq!(runner.configured_max_stderr_bytes(), Some(19));
        assert_eq!(runner.configured_max_total_output_bytes(), Some(23));
    }

    fn managed_file(scope: &OperationScope, file: File) -> qubit_operation::ResourceRef<Mutex<File>> {
        scope
            .manage(Mutex::new(file))
            .expect("test file should register in the scope")
    }

    #[test]
    fn test_take_output_pipe_reports_each_missing_stream() {
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            let error =
                take_output_pipe::<u8>("command", stream, || None).expect_err("missing output pipe should be reported");

            assert_eq!(error.kind(), CommandErrorKind::ReadOutputFailed);
            assert!(matches!(
                error.reason(),
                CommandErrorReason::ReadOutputFailed {
                    stream: actual,
                    ..
                } if *actual == stream
            ));
        }
    }

    #[test]
    fn test_start_output_reader_maps_each_stream_spawn_failure() {
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            let error = start_output_reader("command", stream, || {
                Err(io::Error::other("injected output worker spawn failure"))
            })
            .expect_err("output worker spawn failure should be mapped");

            assert_eq!(error.kind(), CommandErrorKind::StartOutputThreadFailed);
            assert!(matches!(
                error.reason(),
                CommandErrorReason::StartOutputThreadFailed {
                    stream: actual,
                    ..
                } if *actual == stream
            ));
        }
    }

    #[test]
    fn test_runner_configuration_accessors_and_builders() {
        let default = CommandRunner::new(Duration::from_secs(4));
        assert_eq!(default.configured_timeout(), Some(Duration::from_secs(4)));
        assert_eq!(default.configured_success_exit_codes(), &[0]);
        assert!(default.configured_working_directory().is_none());
        assert!(!default.is_logging_disabled());
        assert!(default.is_output_truncation_failure_enabled());
        assert_eq!(
            default.configured_max_stdout_bytes(),
            Some(super::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
        );
        assert_eq!(
            default.configured_max_stderr_bytes(),
            Some(super::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
        );
        assert_eq!(default.configured_max_total_output_bytes(), None);

        let clock = ManualMonotonicClock::new_shared();
        let timer_domain = clock.now().domain();
        let configured = default
            .working_directory("runner/working-directory")
            .timer(Arc::new(clock.new_timer()))
            .success_exit_code(7)
            .success_exit_codes(&[3, 7])
            .disable_logging(true)
            .fail_on_output_truncation(false)
            .diagnostic_redaction_policy(RedactionPolicy::default())
            .max_stdout_bytes(2)
            .max_stderr_bytes(4)
            .max_output_bytes(6)
            .bounded_output(5)
            .max_total_output_bytes(9);

        assert_eq!(
            configured.configured_working_directory().unwrap(),
            "runner/working-directory"
        );
        assert_eq!(configured.configured_timer().clock().now().domain(), timer_domain);
        assert_eq!(configured.configured_success_exit_codes(), &[3, 7]);
        assert!(configured.is_logging_disabled());
        assert!(configured.is_output_truncation_failure_enabled());
        assert_eq!(
            configured.configured_diagnostic_redaction_policy(),
            &RedactionPolicy::default()
        );
        assert_eq!(configured.configured_max_stdout_bytes(), Some(5));
        assert_eq!(configured.configured_max_stderr_bytes(), Some(5));
        assert_eq!(configured.configured_max_total_output_bytes(), Some(9));

        let unbounded = configured
            .clone()
            .unbounded_output()
            .without_total_output_limit()
            .disable_logging(false)
            .fail_on_output_truncation(false);
        assert_eq!(unbounded.configured_max_stdout_bytes(), None);
        assert_eq!(unbounded.configured_max_stderr_bytes(), None);
        assert_eq!(unbounded.configured_max_total_output_bytes(), None);
        assert!(!unbounded.is_logging_disabled());
        assert!(!unbounded.is_output_truncation_failure_enabled());
        assert_eq!(configured.configured_max_stdout_bytes(), Some(5));
        assert_eq!(configured.configured_max_total_output_bytes(), Some(9));

        let untimed = CommandRunner::without_timeout();
        assert_eq!(untimed.configured_timeout(), None);
    }

    #[cfg(unix)]
    fn successful_test_command() -> crate::Command {
        crate::Command::new("sh").arg("-c").arg("printf unit-runner")
    }

    #[cfg(windows)]
    fn successful_test_command() -> crate::Command {
        crate::Command::new("cmd").arg("/C").arg("echo unit-runner")
    }

    #[test]
    fn test_runner_run_and_run_with_return_captured_successful_output() {
        let output = CommandRunner::without_timeout()
            .run(successful_test_command())
            .expect("run should accept a default successful exit code");
        assert_eq!(output.exit_code(), Some(0));
        #[cfg(unix)]
        assert_eq!(output.stdout(), b"unit-runner");
        #[cfg(windows)]
        assert_eq!(output.stdout_text().unwrap().trim(), "unit-runner");

        let output = CommandRunner::new(Duration::from_secs(2))
            .run_with(successful_test_command(), crate::CommandRunOptions::new())
            .expect("run_with should accept explicit per-run options");
        assert_eq!(output.exit_code(), Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn test_runner_applies_working_directory_and_configured_success_codes() {
        let directory = tempfile::tempdir().expect("working directory should be created");
        let output = CommandRunner::without_timeout()
            .working_directory(directory.path())
            .success_exit_codes(&[7])
            .run(crate::Command::new("sh").arg("-c").arg("printf '%s' \"$PWD\"; exit 7"))
            .expect("configured nonzero exit code should be successful");

        assert_eq!(output.exit_code(), Some(7));
        let actual_directory = directory
            .path()
            .canonicalize()
            .expect("working directory should resolve to an absolute path");
        assert_eq!(output.stdout(), actual_directory.to_string_lossy().as_bytes());
    }

    #[cfg(unix)]
    #[test]
    fn test_run_with_preserves_captured_output_for_unexpected_exit() {
        let error = CommandRunner::without_timeout()
            .success_exit_codes(&[0, 7])
            .run_with(
                crate::Command::new("sh")
                    .arg("-c")
                    .arg("printf stdout; printf stderr >&2; exit 9"),
                crate::CommandRunOptions::new(),
            )
            .expect_err("exit code outside the configured success set should fail");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert!(matches!(
            error.reason(),
            CommandErrorReason::UnexpectedExit {
                exit_code: Some(9),
                expected,
            } if expected == &[0, 7]
        ));
        let output = error.output().expect("failed command output should be retained");
        assert_eq!(output.stdout(), b"stdout");
        assert_eq!(output.stderr(), b"stderr");
    }

    #[test]
    fn test_runner_debug_redacts_working_directory() {
        let runner = CommandRunner::without_timeout().working_directory("/private/runner/path");

        let debug = format!("{runner:?}");

        assert!(debug.contains("<redacted path>"));
        assert!(!debug.contains("/private/runner/path"));
    }

    #[test]
    fn test_run_with_rejects_pre_cancelled_command_before_spawn() {
        let cancellation = CommandCancellation::new();
        cancellation.cancel();

        let error = CommandRunner::without_timeout()
            .run_with(
                crate::Command::new("__qubit_cancelled_command_must_not_spawn__"),
                CommandRunOptions::new().cancellation(cancellation),
            )
            .expect_err("a pre-cancelled command should be rejected before spawn");

        assert_eq!(error.kind(), CommandErrorKind::CancelledBeforeStart);
    }

    #[cfg(unix)]
    #[test]
    fn test_runner_enforces_combined_output_budget_on_real_command() {
        let error = CommandRunner::without_timeout()
            .max_total_output_bytes(1)
            .run(crate::Command::new("sh").arg("-c").arg("printf budget"))
            .expect_err("combined stdout and stderr should exceed the configured byte budget");

        assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
        assert!(matches!(
            error.reason(),
            CommandErrorReason::OutputLimitExceeded {
                limit: 1,
                observed
            } if *observed > 1
        ));
        assert!(error.output().is_none());
        assert!(error.captured_output().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_runner_truncation_policy_controls_successful_command_result() {
        let command = || crate::Command::new("sh").arg("-c").arg("printf truncate-me");
        let error = CommandRunner::without_timeout()
            .max_stdout_bytes(3)
            .fail_on_output_truncation(true)
            .run(command())
            .expect_err("enabled truncation failure should reject truncated output");
        assert_eq!(error.kind(), CommandErrorKind::OutputTruncated);
        assert!(error.captured_output().is_some());

        let output = CommandRunner::without_timeout()
            .max_stdout_bytes(3)
            .fail_on_output_truncation(false)
            .run(command())
            .expect("disabled truncation failure should return captured output");
        assert_eq!(output.stdout(), b"tru");
        assert!(output.stdout_truncated());
    }

    #[test]
    fn test_validate_prepared_files_rejects_owned_file_aliases_and_path_tees() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let outcome = manager
            .run(qubit_operation::OperationOptions::default(), |scope| {
                let shared = managed_file(&scope, tempfile::tempfile().expect("fixture file should open"));
                let same_stdin_and_stdout = PreparedFiles::new(
                    Some(shared.clone()),
                    Some(PreparedSink {
                        file: shared.clone(),
                        policy: FileSinkPolicy::Preserve,
                    }),
                    None,
                );
                let error = validate_prepared_files(
                    "test",
                    &same_stdin_and_stdout,
                    &prepared_command("unused-test-program", None, None),
                )
                .expect_err("stdin and stdout must not identify one file");
                assert!(matches!(
                    error.reason(),
                    CommandErrorReason::WriteOutputFailed {
                        stream: OutputStream::Stdout,
                        ..
                    }
                ));

                let same_outputs = PreparedFiles::new(
                    None,
                    Some(PreparedSink {
                        file: shared.clone(),
                        policy: FileSinkPolicy::Preserve,
                    }),
                    Some(PreparedSink {
                        file: shared.clone(),
                        policy: FileSinkPolicy::Preserve,
                    }),
                );
                let error = validate_prepared_files(
                    "test",
                    &same_outputs,
                    &prepared_command("unused-test-program", None, None),
                )
                .expect_err("stdout and stderr must not identify one file");
                assert!(matches!(
                    error.reason(),
                    CommandErrorReason::WriteOutputFailed {
                        stream: OutputStream::Stderr,
                        ..
                    }
                ));

                let same_stdin_and_stderr = PreparedFiles::new(
                    Some(shared.clone()),
                    None,
                    Some(PreparedSink {
                        file: shared.clone(),
                        policy: FileSinkPolicy::Preserve,
                    }),
                );
                let error = validate_prepared_files(
                    "test",
                    &same_stdin_and_stderr,
                    &prepared_command("unused-test-program", None, None),
                )
                .expect_err("stdin and stderr must not identify one file");
                assert!(matches!(
                    error.reason(),
                    CommandErrorReason::WriteOutputFailed {
                        stream: OutputStream::Stdout,
                        ..
                    }
                ));

                let path_tee = tempfile::NamedTempFile::new().expect("tee path should be created");
                let owned_output = PreparedFiles::new(
                    None,
                    Some(PreparedSink {
                        file: shared.clone(),
                        policy: FileSinkPolicy::Preserve,
                    }),
                    None,
                );
                let error = validate_prepared_files(
                    "test",
                    &owned_output,
                    &prepared_command("unused-test-program", Some(path_tee.path()), None),
                )
                .expect_err("owned sinks cannot be combined with path tees");
                assert!(matches!(
                    error.reason(),
                    CommandErrorReason::WriteOutputFailed {
                        stream: OutputStream::Stdout,
                        ..
                    }
                ));

                let stderr_with_path = PreparedFiles::new(
                    None,
                    None,
                    Some(PreparedSink {
                        file: shared.clone(),
                        policy: FileSinkPolicy::Preserve,
                    }),
                );
                let error = validate_prepared_files(
                    "test",
                    &stderr_with_path,
                    &prepared_command("unused-test-program", None, Some(path_tee.path())),
                )
                .expect_err("an owned stderr sink cannot be combined with a path tee");
                assert!(matches!(
                    error.reason(),
                    CommandErrorReason::WriteOutputFailed {
                        stream: OutputStream::Stdout,
                        ..
                    }
                ));
                Ok::<_, std::convert::Infallible>(())
            })
            .expect("validation test operation should finish");
        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
    }

    #[cfg(unix)]
    #[test]
    fn test_apply_prepared_files_uses_stdin_and_sink_dispositions() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let outcome = manager
            .run(qubit_operation::OperationOptions::default(), |scope| {
                let mut stdin = tempfile::tempfile().expect("stdin fixture should open");
                stdin.write_all(b"owned stdin").expect("stdin bytes should write");
                stdin.seek(SeekFrom::Start(0)).expect("stdin should rewind");
                let stdin = managed_file(&scope, stdin);

                let mut stdout = tempfile::tempfile().expect("stdout fixture should open");
                stdout.write_all(b"replace me").expect("stdout prefix should write");
                let stdout = managed_file(&scope, stdout);
                let mut stderr = tempfile::tempfile().expect("stderr fixture should open");
                stderr.write_all(b"preserve").expect("stderr prefix should write");
                let stderr = managed_file(&scope, stderr);
                let files = PreparedFiles::new(
                    Some(stdin),
                    Some(PreparedSink {
                        file: stdout.clone(),
                        policy: FileSinkPolicy::Truncate,
                    }),
                    Some(PreparedSink {
                        file: stderr.clone(),
                        policy: FileSinkPolicy::Preserve,
                    }),
                );
                validate_prepared_files("cat", &files, &prepared_command("cat", None, None))
                    .expect("distinct owned files should validate");
                let mut committed = prepared_command("cat", None, None)
                    .commit()
                    .expect("prepared command should commit");
                apply_prepared_files("cat", &files, &mut committed).expect("owned resources should be installed");

                let output = committed
                    .process_command
                    .arg("-")
                    .output()
                    .expect("stdin file should be connected to the command");
                assert_eq!(output.stdout, b"owned stdin");

                let stdout_tee = committed.stdout_tee.as_mut().expect("stdout tee should be installed");
                stdout_tee
                    .file()
                    .write_all(b"fresh stdout")
                    .expect("stdout tee should write");
                let stderr_tee = committed.stderr_tee.as_mut().expect("stderr tee should be installed");
                stderr_tee
                    .file()
                    .write_all(b" appended")
                    .expect("stderr tee should write");

                let mut actual_stdout = stdout.lock().expect("stdout fixture should lock");
                actual_stdout.seek(SeekFrom::Start(0)).expect("stdout should rewind");
                let mut stdout_bytes = Vec::new();
                actual_stdout
                    .read_to_end(&mut stdout_bytes)
                    .expect("stdout should read");
                assert_eq!(stdout_bytes, b"fresh stdout");

                let mut actual_stderr = stderr.lock().expect("stderr fixture should lock");
                actual_stderr.seek(SeekFrom::Start(0)).expect("stderr should rewind");
                let mut stderr_bytes = Vec::new();
                actual_stderr
                    .read_to_end(&mut stderr_bytes)
                    .expect("stderr should read");
                assert_eq!(stderr_bytes, b"preserve appended");
                Ok::<_, std::convert::Infallible>(())
            })
            .expect("apply test operation should finish");
        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
    }
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
/// [`CommandRunner::unbounded_output`] to remove the per-stream memory limits.
/// Use [`CommandOutput::stdout_text`] and [`CommandOutput::stderr_text`] for
/// strict UTF-8 text, or [`CommandOutput::stdout_lossy_text`] and
/// [`CommandOutput::stderr_lossy_text`] when invalid UTF-8 should be replaced.
/// Timeout, cancellation, and total-output-budget monitoring use timer-based
/// polling. Even when only a total output budget is configured, the timer
/// backend must keep progressing independently while this thread is parked.
/// The runner polls completed stdout/stderr readers while waiting for the child
/// and for inherited output pipes after child exit. If a read failure is
/// observed before another terminal event, the runner stops the child; after
/// successful termination it returns the collector's original
/// `ReadOutputFailed` error with retained partial output. If termination fails,
/// the process-control error remains primary and reader errors remain cleanup
/// details. An earlier tee write failure on that stream remains primary
/// under the existing collector ordering. Budget, child-wait, cancellation,
/// timeout, and clock events keep their existing priority once selected. With
/// no monitoring policy, the runner uses an adaptive 1–10 ms polling sleep and
/// stops only the direct child; a reader failure does not by itself enable
/// process-tree management. A read failure observed before the deadline can
/// change a previous `TimedOut` result into `ReadOutputFailed`. Check the
/// primary error, cleanup failures, and stream-completeness flags when using
/// partial output. Process termination and ordinary-file I/O are not
/// hard-deadline operations.
///
/// # Examples
///
/// ```rust
/// use qubit_command::{Command, CommandRunner};
///
/// let output = CommandRunner::without_timeout()
///     .run(Command::new("rustc").arg("--version"))
///     .expect("rustc should run");
/// assert!(!output.stdout().is_empty());
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
    /// Optional combined stdout/stderr hard budget.
    max_total_output_bytes: Option<u64>,
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
            .field("diagnostic_redaction_policy", &self.diagnostic_redaction_policy)
            .field("max_stdout_bytes", &self.max_stdout_bytes)
            .field("max_stderr_bytes", &self.max_stderr_bytes)
            .field("max_total_output_bytes", &self.max_total_output_bytes)
            .finish()
    }
}

impl CommandRunner {
    /// Creates a command runner with explicit timeout handling configuration.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Optional duration after which a running command is timed
    ///   out.
    #[inline]
    fn with_optional_timeout(timeout: Option<Duration>) -> Self {
        Self {
            timeout,
            timer: Arc::new(StdTimer::new()),
            working_directory: None,
            success_exit_codes: vec![0],
            disable_logging: false,
            fail_on_output_truncation: true,
            diagnostic_redaction_policy: Redactor::application_default().policy().clone(),
            max_stdout_bytes: Some(DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
            max_stderr_bytes: Some(DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM),
            max_total_output_bytes: None,
        }
    }

    /// Creates a command runner with a post-spawn timeout.
    ///
    /// # Parameters
    ///
    /// * `timeout` - Maximum duration allowed after the child process starts.
    ///
    /// # Returns
    ///
    /// A runner that uses a timeout and inherits default command policies.
    #[inline]
    pub fn new(timeout: Duration) -> Self {
        Self::with_optional_timeout(Some(timeout))
    }

    /// Creates a command runner with timeout handling disabled.
    ///
    /// # Returns
    ///
    /// A runner that performs no timeout checks and inherits default command
    /// policies.
    #[inline]
    pub fn without_timeout() -> Self {
        Self::with_optional_timeout(None)
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
    /// Returns a [`CommandError`] with kind
    /// [`CommandErrorKind::CancelledBeforeStart`](crate::CommandErrorKind::CancelledBeforeStart)
    /// when a configured
    /// cancellation handle has already been requested before command
    /// preparation, and maps all process, I/O, and timeout failures as
    /// described by [`CommandRunner::run`].
    pub fn run_with(&self, command: Command, options: CommandRunOptions) -> Result<CommandOutput, CommandError> {
        self.run_with_tree_policy(command, options, RunTreePolicy::default())
    }

    /// Runs a command with an explicit process-tree policy for managed callers.
    ///
    /// # Parameters
    ///
    /// * `command` - Structured command to execute.
    /// * `options` - Per-run cancellation and output configuration.
    /// * `tree_policy` - Managed process and resource ownership supplied by the
    ///   executor.
    ///
    /// # Returns
    ///
    /// Captured output when the process exits with a configured success code.
    ///
    /// # Errors
    ///
    /// Returns the command failure with captured output and cleanup failures.
    pub(crate) fn run_with_tree_policy(
        &self,
        command: Command,
        options: CommandRunOptions,
        tree_policy: RunTreePolicy<'_>,
    ) -> Result<CommandOutput, CommandError> {
        let RunTreePolicy {
            managed_policy,
            start_gate,
            prepared_files,
            capacity_lease,
            reaper_observer,
            session_bridge,
            operation_context,
        } = tree_policy;
        #[cfg(not(any(unix, windows)))]
        if managed_policy == Some(ProcessTreePolicy::Managed) {
            let diagnostic = command.display_command(&self.diagnostic_redaction_policy);
            return Err(spawn_failed(
                &diagnostic,
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "managed process-tree termination is unsupported on this platform",
                ),
            ));
        }
        let CommandRunOptionsParts {
            cancellation,
            stdout_file,
            stderr_file,
            process_tree_policy: ignored_managed_policy,
        } = options.into_parts();
        let _ = ignored_managed_policy;

        let prepared = self.prepare_command_for_run(
            command,
            cancellation.as_ref(),
            stdout_file.as_deref(),
            stderr_file.as_deref(),
            session_bridge.is_some(),
        )?;
        if cancellation.as_ref().is_some_and(CommandCancellation::is_cancelled) {
            return Err(CommandError::from_reason(
                prepared.command_text,
                CommandErrorReason::CancelledBeforeStart,
                None,
            ));
        }
        if let Some(files) = prepared_files.as_ref() {
            validate_prepared_files(&prepared.command_text, files, &prepared)?;
        }
        let _start_permit = match start_gate {
            Some(gate) => match gate.authorize() {
                Some(permit) => Some(permit),
                None => {
                    return Err(CommandError::from_reason(
                        prepared.command_text,
                        CommandErrorReason::CancelledBeforeStart,
                        None,
                    ));
                }
            },
            None => None,
        };
        if let Some(context) = operation_context.as_ref()
            && context.is_expired()
        {
            context.request_stop(qubit_operation::StopReason::Timeout);
            return Err(CommandError::from_reason(
                prepared.command_text,
                CommandErrorReason::CancelledBeforeStart,
                None,
            ));
        }
        let command_diagnostic = prepared.command_text.clone();
        let mut committed = prepared.commit()?;
        if let Some(files) = prepared_files.as_ref() {
            apply_prepared_files(&command_diagnostic, files, &mut committed)?;
        }
        let CommittedCommand {
            command_text,
            process_command,
            stdin_bytes,
            stdout_tee,
            stderr_tee,
        } = committed;

        if !self.disable_logging {
            log::debug!("Running command: {command_text}");
        }

        let manage_process_tree = managed_policy == Some(ProcessTreePolicy::Managed)
            || (managed_policy != Some(ProcessTreePolicy::DirectChild)
                && (self.timeout.is_some() || cancellation.is_some() || self.max_total_output_bytes.is_some()));
        let child_process = match spawn_child(process_command, manage_process_tree) {
            Ok(child_process) => child_process,
            Err(source) => return Err(spawn_failed(&command_text, source)),
        };
        let mut starting_command = StartingCommand::new_managed(
            &command_text,
            child_process,
            capacity_lease,
            reaper_observer,
            session_bridge.clone(),
        );
        let total_budget = self
            .max_total_output_bytes
            .map(|limit| TotalOutputBudget::new(limit, self.max_stdout_bytes, self.max_stderr_bytes))
            .map(Arc::new);
        let started_at = self.timer.clock().now();

        let stdin_writer = match write_stdin_session(
            &command_text,
            starting_command.child_process(),
            stdin_bytes,
            session_bridge.clone(),
        ) {
            Ok(value) => value,
            Err(error) => return Err(starting_command.abort_managed(error)),
        };
        starting_command.set_stdin_writer(stdin_writer);

        let stdout = match take_output_pipe(&command_text, OutputStream::Stdout, || {
            starting_command.child_process().stdout().take()
        }) {
            Ok(value) => value,
            Err(error) => return Err(starting_command.abort_managed(error)),
        };
        let stderr = match take_output_pipe(&command_text, OutputStream::Stderr, || {
            starting_command.child_process().stderr().take()
        }) {
            Ok(value) => value,
            Err(error) => return Err(starting_command.abort_managed(error)),
        };
        let stdout_reader = match start_output_reader(&command_text, OutputStream::Stdout, || {
            read_output_stream(stdout, {
                let mut options =
                    OutputCaptureOptions::new(self.max_stdout_bytes, stdout_tee.map(FileTee::into_output_tee));
                if let Some(bridge) = session_bridge.as_ref() {
                    options = options.with_session_bridge(Arc::clone(bridge), OutputStream::Stdout);
                }
                if let Some(budget) = total_budget.as_ref() {
                    options.with_total_budget(Arc::clone(budget), OutputStream::Stdout)
                } else {
                    options
                }
            })
        }) {
            Ok(value) => value,
            Err(error) => return Err(starting_command.abort_managed(error)),
        };
        starting_command.set_stdout_reader(stdout_reader);
        let stderr_reader = match start_output_reader(&command_text, OutputStream::Stderr, || {
            read_output_stream(stderr, {
                let mut options =
                    OutputCaptureOptions::new(self.max_stderr_bytes, stderr_tee.map(FileTee::into_output_tee));
                if let Some(bridge) = session_bridge.as_ref() {
                    options = options.with_session_bridge(Arc::clone(bridge), OutputStream::Stderr);
                }
                if let Some(budget) = total_budget.as_ref() {
                    options.with_total_budget(Arc::clone(budget), OutputStream::Stderr)
                } else {
                    options
                }
            })
        }) {
            Ok(value) => value,
            Err(error) => return Err(starting_command.abort_managed(error)),
        };
        starting_command.set_stderr_reader(stderr_reader);
        if let Err(source) = self.timer.clock().now().duration_since(started_at) {
            let error =
                CommandError::from_reason(command_text.clone(), CommandErrorReason::TimeFailed { source }, None);
            return Err(starting_command.abort_managed(error));
        }

        let finished_start = starting_command.finish();
        let child_process = finished_start.child_process;
        let command_io = finished_start
            .io
            .with_session_bridge(finished_start.session_bridge.clone());
        let finished = RunningCommand::new_with_reaper(
            command_text,
            child_process,
            command_io,
            started_at,
            Arc::clone(&self.timer),
            RunningCommandResources {
                cancellation_token: cancellation,
                total_budget,
                capacity_lease: finished_start.capacity_lease,
                reaper_observer: finished_start.reaper_observer,
                session_bridge: finished_start.session_bridge,
                operation_context,
            },
        )
        .wait_for_completion(self.timeout)?;
        let FinishedCommand { command_text, output } = finished;

        if output
            .exit_code()
            .is_some_and(|exit_code| self.success_exit_codes.contains(&exit_code))
        {
            if self.fail_on_output_truncation && (output.stdout_truncated() || output.stderr_truncated()) {
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
                log::debug!("Finished command `{}` in {:?}.", command_text, output.elapsed());
            }
            Ok(output)
        } else {
            if !self.disable_logging {
                log::debug!("Command `{}` exited with code {:?}.", command_text, output.exit_code());
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
    /// Timer used for elapsed measurement and blocking sleeps during timeout,
    /// cancellation, or total-output-budget monitoring.
    #[must_use]
    #[inline]
    pub fn configured_timer(&self) -> &dyn Timer {
        self.timer.as_ref()
    }

    /// Replaces the monotonic timer used for elapsed measurement and blocking
    /// sleeps during timeout, cancellation, or total-output-budget monitoring.
    ///
    /// Command execution waits on this timer synchronously. Its backend must
    /// therefore make progress independently of the caller thread, including
    /// when a total output budget is the only monitoring policy. In particular,
    /// do not use a Tokio timer whose only driver is the same
    /// current-thread runtime that calls [`Self::run`].
    ///
    /// # Parameters
    ///
    /// * `timer` - Timer whose clock measures elapsed command time.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
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
    #[inline]
    pub fn configured_working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    /// Sets the default working directory.
    ///
    /// This directory affects only the child process. Relative stdin and tee
    /// file paths are resolved from the parent process current working
    /// directory. To access a file relative to this directory, pass an
    /// explicitly joined absolute path.
    ///
    /// # Type Parameters
    ///
    /// * `P` - Path-like value converted into an owned working-directory path.
    ///
    /// # Parameters
    ///
    /// * `working_directory` - Directory used when a command has no per-command
    ///   working directory override.
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

    /// Returns the configured successful exit codes.
    ///
    /// # Returns
    ///
    /// Borrowed list of exit codes treated as successful.
    #[must_use]
    #[inline]
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
    #[inline]
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
    #[inline]
    pub const fn disable_logging(mut self, disable_logging: bool) -> Self {
        self.disable_logging = disable_logging;
        self
    }

    /// Returns whether truncated output makes a successful command fail.
    ///
    /// # Returns
    ///
    /// `true` when a successful command returns
    /// [`CommandErrorKind::OutputTruncated`](crate::CommandErrorKind::OutputTruncated) if either
    /// captured stream is truncated.
    #[must_use]
    #[inline]
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
    #[inline]
    pub const fn fail_on_output_truncation(mut self, fail_on_output_truncation: bool) -> Self {
        self.fail_on_output_truncation = fail_on_output_truncation;
        self
    }

    /// Returns the immutable policy used for command diagnostics and logs.
    ///
    /// # Returns
    ///
    /// The complete configured diagnostic redaction policy.
    #[must_use]
    #[inline]
    pub const fn configured_diagnostic_redaction_policy(&self) -> &RedactionPolicy {
        &self.diagnostic_redaction_policy
    }

    /// Replaces the complete policy used for command diagnostics and logs.
    ///
    /// The policy affects runner lifecycle logs and
    /// [`CommandError::command`]. Standalone [`Command`]
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
    #[inline]
    pub fn diagnostic_redaction_policy(mut self, policy: RedactionPolicy) -> Self {
        self.diagnostic_redaction_policy = policy;
        self
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

    /// Sets the maximum stdout bytes retained in memory.
    ///
    /// A per-stream retention limit alone does not stop draining stdout or
    /// writing to its configured tee file. A total output budget, timeout,
    /// cancellation, or a pipe read failure can stop collection. A tee write
    /// failure can leave the tee incomplete while draining continues; a tee
    /// flush failure can leave it incomplete after the stream was drained.
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

    /// Returns the configured stderr capture limit.
    ///
    /// # Returns
    ///
    /// `Some(max_bytes)` when stderr capture is limited, otherwise `None`.
    #[inline]
    pub const fn configured_max_stderr_bytes(&self) -> Option<usize> {
        self.max_stderr_bytes
    }

    /// Sets the maximum stderr bytes retained in memory.
    ///
    /// A per-stream retention limit alone does not stop draining stderr or
    /// writing to its configured tee file. A total output budget, timeout,
    /// cancellation, or a pipe read failure can stop collection. A tee write
    /// failure can leave the tee incomplete while draining continues; a tee
    /// flush failure can leave it incomplete after the stream was drained.
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

    /// Limits both captured streams and rejects any truncated result.
    ///
    /// This also enables failure on output truncation, overriding a prior
    /// [`Self::fail_on_output_truncation`] setting. The order of builder calls
    /// therefore matters. This changes only per-stream retention limits and
    /// truncation handling; it does not change the combined output budget.
    ///
    /// # Parameters
    ///
    /// * `max_bytes` - Maximum number of retained bytes for each stream.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn bounded_output(mut self, max_bytes: usize) -> Self {
        self.max_stdout_bytes = Some(max_bytes);
        self.max_stderr_bytes = Some(max_bytes);
        self.fail_on_output_truncation = true;
        self
    }

    /// Removes both per-stream in-memory capture limits and accepts truncated
    /// output.
    ///
    /// This also disables failure on output truncation, overriding a prior
    /// [`Self::fail_on_output_truncation`] setting. The order of builder calls
    /// therefore matters. It does not remove a configured combined output
    /// budget. Without one, the child can consume unbounded memory through
    /// stdout or stderr.
    ///
    /// # Returns
    ///
    /// The updated command runner.
    #[inline]
    pub const fn unbounded_output(mut self) -> Self {
        self.max_stdout_bytes = None;
        self.max_stderr_bytes = None;
        self.fail_on_output_truncation = false;
        self
    }

    /// Returns the configured combined stdout/stderr hard byte limit.
    #[inline]
    pub const fn configured_max_total_output_bytes(&self) -> Option<u64> {
        self.max_total_output_bytes
    }

    /// Sets a hard byte limit shared by stdout and stderr for each run.
    ///
    /// Stdout and stderr share this per-run budget. Bytes count even when they
    /// are not retained in memory or are sent to a tee. The runner observes at
    /// most `limit + 1` bytes; the extra byte proves that the limit was
    /// exceeded. A zero limit therefore permits empty output, while the first
    /// output byte exceeds it.
    ///
    /// A configured total budget uses timer-based polling even when timeout
    /// and cancellation are disabled; the timer must progress independently
    /// while synchronous `run` blocks. The budget is a byte limit, not a hard
    /// wall-clock deadline, and does not bound preparation, spawning, or
    /// arbitrary filesystem operations. Exceeding the limit requests
    /// termination of the managed process tree,
    /// including when no timeout or cancellation is configured. Termination
    /// follows the runner's existing best-effort cleanup contract and does not
    /// impose a hard wall-clock deadline. The resulting
    /// [`CommandErrorReason::OutputLimitExceeded`] includes the limit and
    /// observed byte count. [`CommandError::output`] returns `None`, while
    /// [`CommandError::captured_output`] may return incomplete raw bytes.
    /// Per-stream retention limits remain controlled by
    /// [`Self::max_output_bytes`] or [`Self::bounded_output`]; removing them
    /// with [`Self::unbounded_output`] does not remove this total budget.
    ///
    /// # Parameters
    ///
    /// * `limit` - Maximum combined stdout and stderr bytes observed.
    ///
    /// # Example
    ///
    /// ```
    /// use qubit_command::CommandError;
    /// use qubit_command::CommandErrorKind;
    /// use qubit_command::CommandErrorReason;
    /// use qubit_command::CommandOutput;
    /// use qubit_command::CommandRunner;
    ///
    /// let runner = CommandRunner::without_timeout().max_total_output_bytes(1024);
    /// assert_eq!(runner.configured_max_total_output_bytes(), Some(1024));
    /// fn inspect(result: Result<CommandOutput, CommandError>) {
    ///     if let Err(error) = result
    ///         && error.kind() == CommandErrorKind::OutputLimitExceeded
    ///     {
    ///         assert!(error.output().is_none());
    ///         if let CommandErrorReason::OutputLimitExceeded { limit, observed } = error.reason() {
    ///             assert!(*observed > u128::from(*limit));
    ///         }
    ///         if let Some(bytes) = error.captured_output() {
    ///             let _ = (bytes.stdout(), bytes.stderr(), bytes.stdout_complete(), bytes.stderr_complete());
    ///         }
    ///     }
    /// }
    /// ```
    #[inline]
    pub const fn max_total_output_bytes(mut self, limit: u64) -> Self {
        self.max_total_output_bytes = Some(limit);
        self
    }

    /// Removes the combined stdout/stderr hard byte limit. This is independent
    /// of the in-memory capture limits controlled by
    /// [`Self::unbounded_output`].
    #[inline]
    pub const fn without_total_output_limit(mut self) -> Self {
        self.max_total_output_bytes = None;
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
        session_stdin: bool,
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
            session_stdin,
        )
    }
}
