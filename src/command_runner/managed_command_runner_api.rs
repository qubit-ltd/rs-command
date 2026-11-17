// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Managed command runner entry points.
use qubit_operation::OperationScope;

use crate::Command;
use crate::CommandError;
use crate::CommandExecutor;
#[cfg(feature = "async")]
use crate::CommandExecutorApi;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
#[cfg(feature = "async")]
use crate::CommandSession;
#[cfg(feature = "async")]
use crate::CommandStderr;
#[cfg(feature = "async")]
use crate::CommandStdin;
#[cfg(feature = "async")]
use crate::CommandStdout;

/// Adds managed executor operations to command runners.
///
/// The methods submit commands to a [`CommandExecutor`] and collect the
/// terminal operation result before returning output or a command session.
pub trait ManagedCommandRunnerApi {
    /// Runs a command through the managed executor and waits for settlement.
    ///
    /// # Parameters
    ///
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed(
        &self,
        executor: &CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    /// Runs a command with executor-prepared file resources.
    ///
    /// # Parameters
    ///
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed_resources(
        &self,
        executor: &CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    /// Runs a command within an existing operation scope.
    ///
    /// # Parameters
    ///
    /// * `scope` - Parent operation scope containing this command.
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed_in(
        &self,
        scope: &OperationScope,
        executor: &CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    /// Runs a resource-backed command within an operation scope.
    ///
    /// # Parameters
    ///
    /// * `scope` - Parent operation scope containing this command.
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed_resources_in(
        &self,
        scope: &OperationScope,
        executor: &CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    /// Submits a command asynchronously and waits for settlement.
    ///
    /// # Parameters
    ///
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn run_async<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    /// Submits a resource-backed command asynchronously.
    ///
    /// # Parameters
    ///
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn run_async_resources<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    /// Starts an asynchronous command session.
    ///
    /// # Parameters
    ///
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn start_session<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<CommandSession, CommandError>> + Send + 'a;

    /// Starts a session using executor-prepared resources.
    ///
    /// # Parameters
    ///
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn start_session_resources<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<CommandSession, CommandError>> + Send + 'a;

    /// Submits a command asynchronously within an operation scope.
    ///
    /// # Parameters
    ///
    /// * `scope` - Parent operation scope containing this command.
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn run_async_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    /// Submits a resource-backed command within an operation scope.
    ///
    /// # Parameters
    ///
    /// * `scope` - Parent operation scope containing this command.
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn run_async_resources_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    /// Starts a command session within an existing operation scope.
    ///
    /// # Parameters
    ///
    /// * `scope` - Parent operation scope containing this command.
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn start_session_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<CommandSession, CommandError>> + Send + 'a;

    /// Starts a resource-backed session within an operation scope.
    ///
    /// # Parameters
    ///
    /// * `scope` - Parent operation scope containing this command.
    /// * `executor` - Executor responsible for the managed operation.
    /// * `command` - Process invocation to run.
    /// * `options` - Run configuration, including any prepared resources.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    fn start_session_resources_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<CommandSession, CommandError>> + Send + 'a;
}

impl ManagedCommandRunnerApi for crate::CommandRunner {
    /// Runs a command through the managed executor and waits for settlement.
    ///
    /// # Parameters
    ///
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed(
        &self,
        e: &CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect(
            e.submit(self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
    }
    /// Runs a command using executor-prepared file resources.
    ///
    /// # Parameters
    ///
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed_resources(
        &self,
        e: &CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect(
            e.submit_resources(self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
    }
    /// Runs a command within the supplied operation scope.
    ///
    /// # Parameters
    ///
    /// * `s` - Parent operation scope containing this command.
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed_in(
        &self,
        s: &OperationScope,
        e: &CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect(
            e.submit_in(s, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
    }
    /// Runs a resource-backed command within the supplied scope.
    ///
    /// # Parameters
    ///
    /// * `s` - Parent operation scope containing this command.
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    fn run_managed_resources_in(
        &self,
        s: &OperationScope,
        e: &CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect(
            e.submit_resources_in(s, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
    }
    /// Submits a command asynchronously and waits for settlement.
    ///
    /// # Parameters
    ///
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn run_async<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            CommandExecutorApi::submit_async(e, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
    /// Submits a resource-backed command asynchronously.
    ///
    /// # Parameters
    ///
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn run_async_resources<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            CommandExecutorApi::submit_async_resources(e, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
    /// Starts a managed asynchronous command session.
    ///
    /// # Parameters
    ///
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn start_session<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<CommandSession, CommandError> {
        e.start_session_managed(self.clone(), c, o)
    }
    /// Starts a session using executor-prepared resources.
    ///
    /// # Parameters
    ///
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn start_session_resources<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<CommandSession, CommandError> {
        e.start_session_resources_managed(self.clone(), c, o)
    }
    /// Submits a command asynchronously within the supplied scope.
    ///
    /// # Parameters
    ///
    /// * `s` - Parent operation scope containing this command.
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn run_async_in<'a>(
        &'a self,
        s: &'a OperationScope,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            CommandExecutorApi::submit_async_in(e, s, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
    /// Submits a resource-backed command within the supplied scope.
    ///
    /// # Parameters
    ///
    /// * `s` - Parent operation scope containing this command.
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The captured output produced by the managed command.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn run_async_resources_in<'a>(
        &'a self,
        s: &'a OperationScope,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            CommandExecutorApi::submit_async_resources_in(e, s, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
    /// Starts a managed session within the supplied scope.
    ///
    /// # Parameters
    ///
    /// * `s` - Parent operation scope containing this command.
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn start_session_in<'a>(
        &'a self,
        s: &'a OperationScope,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<CommandSession, CommandError> {
        e.start_session_managed_in(s, self.clone(), c, o)
    }
    /// Starts a resource-backed session within the supplied scope.
    ///
    /// # Parameters
    ///
    /// * `s` - Parent operation scope containing this command.
    /// * `e` - Executor responsible for the managed operation.
    /// * `c` - Process invocation to run.
    /// * `o` - Per-run configuration.
    ///
    /// # Returns
    ///
    /// The command session provided by the managed executor.
    ///
    /// # Errors
    ///
    /// Returns an error if submission or command execution fails.
    #[cfg(feature = "async")]
    async fn start_session_resources_in<'a>(
        &'a self,
        s: &'a OperationScope,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<CommandSession, CommandError> {
        e.start_session_resources_managed_in(s, self.clone(), c, o)
    }
}

#[cfg(feature = "async")]
impl crate::session::CommandSessionApi for CommandSession {
    /// Borrows the operation associated with this session.
    ///
    /// # Returns
    ///
    /// The operation handle, or `None` if it was already taken.
    fn operation(&self) -> Option<&crate::CommandOperation> {
        self.operation.as_ref()
    }
    /// Removes the operation handle from this session.
    ///
    /// # Returns
    ///
    /// The owned operation handle, or `None` if it was already taken.
    fn take_operation(&mut self) -> Option<crate::CommandOperation> {
        self.operation.take()
    }
    /// Removes the stdin stream from this session.
    ///
    /// # Returns
    ///
    /// The owned stdin stream, or `None` if it was already taken.
    fn take_stdin(&mut self) -> Option<CommandStdin> {
        self.stdin.take()
    }
    /// Removes the stdout stream from this session.
    ///
    /// # Returns
    ///
    /// The owned stdout stream, or `None` if it was already taken.
    fn take_stdout(&mut self) -> Option<CommandStdout> {
        self.stdout.take()
    }
    /// Removes the stderr stream from this session.
    ///
    /// # Returns
    ///
    /// The owned stderr stream, or `None` if it was already taken.
    fn take_stderr(&mut self) -> Option<CommandStderr> {
        self.stderr.take()
    }
}

/// Waits for the operation outcome and its final settlement.
///
/// # Parameters
///
/// * `handle` - Submitted operation whose output is collected.
///
/// # Returns
///
/// The command output when the operation succeeds.
///
/// # Errors
///
/// Returns an error if outcome or settlement is unavailable.
fn collect(mut handle: crate::CommandOperation) -> Result<crate::CommandOutput, CommandError> {
    let outcome = handle
        .wait_outcome()
        .map_err(|e| managed_error(format!("operation outcome unavailable: {e}")))?;
    let _settlement = handle
        .wait_settlement()
        .map_err(|e| managed_error(format!("operation settlement unavailable: {e}")))?;
    map_outcome(outcome)
}
/// Asynchronously waits for operation completion and settlement.
///
/// # Parameters
///
/// * `handle` - Submitted asynchronous operation whose output is collected.
///
/// # Returns
///
/// The command output when the operation succeeds.
///
/// # Errors
///
/// Returns an error if the outcome cannot be read.
#[cfg(feature = "async")]
async fn collect_async(mut handle: crate::AsyncCommandOperation) -> Result<crate::CommandOutput, CommandError> {
    let outcome = handle
        .wait_outcome()
        .await
        .map_err(|e| managed_error(format!("operation outcome unavailable: {e}")))?;
    let _settlement = handle.wait_settlement().await;
    map_outcome(outcome)
}
/// Converts executor lifecycle outcomes into the command API result.
///
/// # Parameters
///
/// * `outcome` - Terminal state reported by the managed operation.
///
/// # Returns
///
/// The successful output or a command error for the terminal failure.
fn map_outcome(
    outcome: qubit_operation::OperationOutcome<crate::CommandOutput, CommandError>,
) -> Result<crate::CommandOutput, CommandError> {
    match outcome {
        qubit_operation::OperationOutcome::Succeeded(value) => Ok(value),
        qubit_operation::OperationOutcome::Failed(error) => Err(error),
        qubit_operation::OperationOutcome::Stopped(_) => Err(CommandError::from_reason(
            "<managed command>",
            crate::CommandErrorReason::Cancelled,
            None,
        )),
        qubit_operation::OperationOutcome::ChildFailed(failure) => {
            Err(managed_error(failure.outcome.summary.as_str().to_owned()))
        }
        qubit_operation::OperationOutcome::Panicked(panic) => Err(managed_error(panic.summary.as_str().to_owned())),
        qubit_operation::OperationOutcome::ExecutionFailed(failure) => {
            Err(managed_error(failure.summary.as_str().to_owned()))
        }
    }
}
/// Creates an error for failures in managed operation handling.
///
/// # Parameters
///
/// * `message` - Diagnostic describing the managed failure.
///
/// # Returns
///
/// An error classified as a managed operation failure.
fn managed_error(message: String) -> CommandError {
    CommandError::from_reason(
        "<managed command>",
        crate::CommandErrorReason::ManagedOperationFailed { diagnostic: message },
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use super::ManagedCommandRunnerApi;
    use crate::Command;
    use crate::CommandExecutionConfig;
    use crate::CommandExecutor;
    use crate::CommandResourceRunOptions;
    use crate::CommandRunner;

    fn setup() -> (OperationManager, CommandExecutor, CommandRunner) {
        let manager =
            OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let executor = CommandExecutor::new(manager.clone(), CommandExecutionConfig::default())
            .expect("executor should start");
        let runner = CommandRunner::new(Duration::from_secs(5));
        (manager, executor, runner)
    }

    #[cfg(unix)]
    #[test]
    fn sync_runner_methods_collect_root_and_scoped_command_results() {
        let (manager, executor, runner) = setup();
        let root = runner
            .run_managed(
                &executor,
                Command::new("sh").arg("-c").arg("printf root"),
                Default::default(),
            )
            .expect("managed command should succeed");
        assert_eq!(root.stdout(), b"root");

        let mut input = tempfile::tempfile().expect("stdin file should be created");
        use std::io::Seek;
        use std::io::Write;
        input.write_all(b"owned-input").unwrap();
        input.rewind().unwrap();
        let resources = runner
            .run_managed_resources(
                &executor,
                Command::new("sh").arg("-c").arg("cat"),
                CommandResourceRunOptions::new().stdin_file(input),
            )
            .expect("resource-backed command should succeed");
        assert_eq!(resources.stdout(), b"owned-input");

        let scoped_executor = executor.clone();
        let scoped_runner = runner.clone();
        let result = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let ordinary = scoped_runner.run_managed_in(
                    &scope,
                    &scoped_executor,
                    Command::new("sh").arg("-c").arg("printf scoped"),
                    Default::default(),
                )?;
                let resources = scoped_runner.run_managed_resources_in(
                    &scope,
                    &scoped_executor,
                    Command::new("sh").arg("-c").arg("printf scoped-resource"),
                    CommandResourceRunOptions::new(),
                )?;
                assert_eq!(resources.stdout(), b"scoped-resource");
                Ok::<_, crate::CommandError>(ordinary)
            })
            .expect("parent operation should complete");
        match result {
            OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"scoped"),
            other => panic!("scoped runner call failed: {other:?}"),
        }

        let error = runner
            .run_managed(
                &executor,
                Command::new("__qubit_managed_runner_missing_executable__"),
                Default::default(),
            )
            .expect_err("spawn failure should be returned to the caller");
        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::SpawnFailed { .. }
        ));
    }

    #[cfg(all(feature = "async", unix))]
    mod async_tests {
        use std::future::Future;
        use std::io::Seek;
        use std::io::Write;
        use std::sync::Arc;
        use std::task::Context;
        use std::task::Poll;
        use std::task::Wake;
        use std::task::Waker;

        use super::*;
        use crate::CommandSession;
        use crate::session::CommandSessionApi;
        use qubit_io::AsyncInput;

        struct ThreadWake(std::thread::Thread);
        impl Wake for ThreadWake {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.unpark();
            }
        }

        fn block_on<F: Future>(future: F) -> F::Output {
            let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
            let mut context = Context::from_waker(&waker);
            let mut future = Box::pin(future);
            loop {
                if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                    return value;
                }
                std::thread::park_timeout(Duration::from_secs(2));
            }
        }

        async fn read_all(
            reader: &mut (impl AsyncInput<Item = u8> + Unpin),
        ) -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 128];
            loop {
                let count = reader.read_async(&mut buffer).await?;
                if count == 0 {
                    return Ok(bytes);
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
        }

        async fn session_bytes(mut session: CommandSession) -> (Vec<u8>, Vec<u8>) {
            let stdin = session
                .take_stdin()
                .expect("session stdin should be available");
            drop(stdin);
            let mut stdout = session
                .take_stdout()
                .expect("session stdout should be available");
            let mut stderr = session
                .take_stderr()
                .expect("session stderr should be available");
            let stdout = read_all(&mut stdout)
                .await
                .expect("stdout should be readable");
            let stderr = read_all(&mut stderr)
                .await
                .expect("stderr should be readable");
            let mut operation = session
                .take_operation()
                .expect("session operation should be available");
            assert!(matches!(
                operation
                    .wait_outcome()
                    .expect("session outcome should be available"),
                OperationOutcome::Succeeded(_)
            ));
            operation.wait_settlement().expect("session should settle");
            (stdout, stderr)
        }

        #[test]
        fn async_runner_methods_collect_root_and_scoped_results_and_sessions() {
            let (manager, executor, runner) = setup();
            let root = block_on(runner.run_async(
                &executor,
                Command::new("sh").arg("-c").arg("printf async-root"),
                Default::default(),
            ))
            .expect("async command should succeed");
            assert_eq!(root.stdout(), b"async-root");

            let mut input = tempfile::tempfile().expect("stdin fixture should be created");
            input.write_all(b"async-owned").unwrap();
            input.rewind().unwrap();
            let resources = block_on(runner.run_async_resources(
                &executor,
                Command::new("sh").arg("-c").arg("cat"),
                CommandResourceRunOptions::new().stdin_file(input),
            ))
            .expect("async resource command should succeed");
            assert_eq!(resources.stdout(), b"async-owned");

            let session = block_on(
                runner.start_session(
                    &executor,
                    Command::new("sh")
                        .arg("-c")
                        .arg("printf session-out; printf session-err >&2"),
                    Default::default(),
                ),
            )
            .expect("session should start");
            assert_eq!(
                block_on(session_bytes(session)),
                (b"session-out".to_vec(), b"session-err".to_vec())
            );

            let session = block_on(runner.start_session_resources(
                &executor,
                Command::new("sh").arg("-c").arg("printf resource-session"),
                CommandResourceRunOptions::new(),
            ))
            .expect("resource session should start");
            assert_eq!(block_on(session_bytes(session)).0, b"resource-session");

            let scoped_executor = executor.clone();
            let scoped_runner = runner.clone();
            let outcome = block_on(manager.as_async().run(
                qubit_operation::OperationOptions::default(),
                move |scope| async move {
                    let ordinary = scoped_runner
                        .run_async_in(
                            &scope,
                            &scoped_executor,
                            Command::new("sh").arg("-c").arg("printf async-scoped"),
                            Default::default(),
                        )
                        .await?;
                    let resources = scoped_runner
                        .run_async_resources_in(
                            &scope,
                            &scoped_executor,
                            Command::new("sh")
                                .arg("-c")
                                .arg("printf async-scoped-resource"),
                            CommandResourceRunOptions::new(),
                        )
                        .await?;
                    assert_eq!(resources.stdout(), b"async-scoped-resource");
                    let session = scoped_runner
                        .start_session_in(
                            &scope,
                            &scoped_executor,
                            Command::new("sh")
                                .arg("-c")
                                .arg("printf async-scoped-session"),
                            Default::default(),
                        )
                        .await?;
                    assert_eq!(session_bytes(session).await.0, b"async-scoped-session");
                    let session = scoped_runner
                        .start_session_resources_in(
                            &scope,
                            &scoped_executor,
                            Command::new("sh")
                                .arg("-c")
                                .arg("printf async-scoped-resource-session"),
                            CommandResourceRunOptions::new(),
                        )
                        .await?;
                    assert_eq!(
                        session_bytes(session).await.0,
                        b"async-scoped-resource-session"
                    );
                    Ok::<_, crate::CommandError>(ordinary)
                },
            ))
            .expect("async parent operation should complete");
            match outcome {
                OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"async-scoped"),
                other => panic!("async scoped runner call failed: {other:?}"),
            }
            let error = block_on(runner.run_async(
                &executor,
                Command::new("__qubit_managed_runner_missing_async_executable__"),
                Default::default(),
            ))
            .expect_err("async spawn failure should be returned");
            assert!(matches!(
                error.reason(),
                crate::CommandErrorReason::SpawnFailed { .. }
            ));
        }
    }
}
