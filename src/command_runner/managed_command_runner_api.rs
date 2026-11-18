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
    let stop_diagnostic = handle.take_stop_diagnostic();
    map_outcome_with_stop_diagnostic(outcome, stop_diagnostic)
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
    let stop_diagnostic = handle.take_stop_diagnostic();
    map_outcome_with_stop_diagnostic(outcome, stop_diagnostic)
}

/// Converts a stopped outcome using its retained command diagnostics when any
/// are available.
///
/// # Parameters
/// * `outcome`: Terminal operation result.
/// * `stop_diagnostic`: Runner error retained alongside a stopped result.
fn map_outcome_with_stop_diagnostic(
    outcome: qubit_operation::OperationOutcome<crate::CommandOutput, CommandError>,
    stop_diagnostic: Option<CommandError>,
) -> Result<crate::CommandOutput, CommandError> {
    match (outcome, stop_diagnostic) {
        (qubit_operation::OperationOutcome::Stopped(report), Some(error)) => {
            Err(error.with_managed_stop_reason(report.reason))
        }
        (outcome, _) => map_outcome(outcome),
    }
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
        qubit_operation::OperationOutcome::Stopped(report) => {
            let reason = match report.reason {
                qubit_operation::StopReason::UserCancelled => crate::CommandErrorReason::Cancelled,
                reason => crate::CommandErrorReason::Stopped { reason },
            };
            Err(CommandError::from_reason("<managed command>", reason, None).with_managed_stop_reason(report.reason))
        }
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
    use super::map_outcome;
    use crate::Command;
    use crate::CommandExecutionConfig;
    use crate::CommandExecutor;
    use crate::CommandResourceRunOptions;
    use crate::CommandRunner;

    #[test]
    fn test_failed_operation_preserves_the_command_error() {
        let error = crate::CommandError::from_reason("tool", crate::CommandErrorReason::Cancelled, None);
        let actual = map_outcome(OperationOutcome::<crate::CommandOutput, _>::Failed(error)).unwrap_err();

        assert_eq!(actual.command(), "tool");
        assert!(matches!(actual.reason(), crate::CommandErrorReason::Cancelled));
    }

    #[test]
    fn test_sync_managed_runner_maps_admission_rejection_for_owned_and_plain_requests() {
        let (_manager, executor, runner) = setup();
        let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);

        let ordinary = runner
            .run_managed(
                &executor,
                Command::new("__must_not_spawn_after_executor_shutdown__"),
                Default::default(),
            )
            .expect_err("closed executor should reject an ordinary request");
        let resources = runner
            .run_managed_resources(
                &executor,
                Command::new("__must_not_spawn_after_executor_shutdown__"),
                CommandResourceRunOptions::new(),
            )
            .expect_err("closed executor should reject an owned-resource request");

        for error in [ordinary, resources] {
            assert_eq!(error.kind(), crate::CommandErrorKind::ManagedOperationFailed);
            assert!(matches!(
                error.reason(),
                crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                    if diagnostic.starts_with("submission rejected:")
            ));
            assert!(error.output().is_none());
        }
    }

    #[test]
    fn test_scoped_sync_managed_runner_maps_wrong_manager_rejection_for_both_request_kinds() {
        let (manager, _executor, runner) = setup();
        let other_manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let wrong_executor =
            CommandExecutor::new(other_manager, CommandExecutionConfig::default()).expect("executor should start");

        let outcome = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let ordinary = runner
                    .run_managed_in(
                        &scope,
                        &wrong_executor,
                        Command::new("__must_not_run_for_wrong_managed_manager__"),
                        Default::default(),
                    )
                    .expect_err("wrong manager should reject a scoped managed request");
                let resources = runner
                    .run_managed_resources_in(
                        &scope,
                        &wrong_executor,
                        Command::new("__must_not_run_for_wrong_managed_manager__"),
                        CommandResourceRunOptions::new(),
                    )
                    .expect_err("wrong manager should reject a scoped resource request");

                for error in [ordinary, resources] {
                    assert_eq!(error.kind(), crate::CommandErrorKind::ManagedOperationFailed);
                    assert!(matches!(
                        error.reason(),
                        crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                            if diagnostic == "submission rejected: GroupBinding(WrongManager)"
                    ));
                    assert!(error.output().is_none());
                }
                Ok::<_, std::convert::Infallible>(())
            })
            .expect("parent operation should complete");
        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
    }

    fn setup() -> (OperationManager, CommandExecutor, CommandRunner) {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let executor =
            CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).expect("executor should start");
        let runner = CommandRunner::new(Duration::from_secs(5));
        (manager, executor, runner)
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_runner_methods_collect_root_and_scoped_command_results() {
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
        assert!(matches!(error.reason(), crate::CommandErrorReason::SpawnFailed { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_managed_runner_preserves_truncation_error_and_captured_output() {
        let (_manager, executor, _) = setup();
        let runner = CommandRunner::without_timeout()
            .max_stdout_bytes(3)
            .fail_on_output_truncation(true);

        let error = runner
            .run_managed(
                &executor,
                Command::new("sh").arg("-c").arg("printf truncate-me"),
                Default::default(),
            )
            .expect_err("managed execution should preserve the configured truncation failure");

        assert_eq!(error.kind(), crate::CommandErrorKind::OutputTruncated);
        assert!(matches!(error.reason(), crate::CommandErrorReason::OutputTruncated));
        let captured = error.captured_output().expect("truncated bytes should be retained");
        assert_eq!(captured.stdout(), b"tru");
        assert!(captured.stdout_truncated());
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
        use std::time::Duration;
        use std::time::Instant;

        use qubit_io::AsyncInput;
        use qubit_operation::ManagerOptions;
        use qubit_operation::OperationManager;
        use qubit_operation::OperationOptions;
        use qubit_operation::OperationOutcome;

        use super::super::collect_async;
        use super::ManagedCommandRunnerApi;
        use super::setup;
        use crate::Command;
        use crate::CommandErrorKind;
        use crate::CommandErrorReason;
        use crate::CommandExecutionConfig;
        use crate::CommandExecutor;
        use crate::CommandResourceRunOptions;
        use crate::CommandSession;
        use crate::session::CommandSessionApi;

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

        async fn read_all(reader: &mut (impl AsyncInput<Item = u8> + Unpin)) -> std::io::Result<Vec<u8>> {
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
            let stdin = session.take_stdin().expect("session stdin should be available");
            drop(stdin);
            let mut stdout = session.take_stdout().expect("session stdout should be available");
            let mut stderr = session.take_stderr().expect("session stderr should be available");
            let stdout = read_all(&mut stdout).await.expect("stdout should be readable");
            let stderr = read_all(&mut stderr).await.expect("stderr should be readable");
            let mut operation = session.take_operation().expect("session operation should be available");
            assert!(matches!(
                operation.wait_outcome().expect("session outcome should be available"),
                OperationOutcome::Succeeded(_)
            ));
            operation.wait_settlement().expect("session should settle");
            (stdout, stderr)
        }

        #[test]
        fn test_async_managed_runner_maps_closed_executor_rejection() {
            let (_manager, executor, runner) = setup();
            let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);

            let ordinary = block_on(runner.run_async(
                &executor,
                Command::new("__must_not_spawn_after_async_executor_shutdown__"),
                Default::default(),
            ))
            .expect_err("closed executor should reject an async request");
            let resources = block_on(runner.run_async_resources(
                &executor,
                Command::new("__must_not_spawn_after_async_executor_shutdown__"),
                CommandResourceRunOptions::new(),
            ))
            .expect_err("closed executor should reject an async resource request");

            for error in [ordinary, resources] {
                assert_eq!(error.kind(), crate::CommandErrorKind::ManagedOperationFailed);
                assert!(matches!(
                    error.reason(),
                    crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                        if diagnostic.starts_with("submission rejected:")
                ));
                assert!(error.output().is_none());
            }
        }

        #[test]
        fn test_managed_session_methods_report_closed_executor_rejection() {
            let (_manager, executor, runner) = setup();
            let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);

            let ordinary = block_on(runner.start_session(
                &executor,
                Command::new("__must_not_start_after_session_executor_shutdown__"),
                Default::default(),
            ))
            .err()
            .expect("closed executor should reject a managed session");
            let resources = block_on(runner.start_session_resources(
                &executor,
                Command::new("__must_not_start_after_session_executor_shutdown__"),
                CommandResourceRunOptions::new(),
            ))
            .err()
            .expect("closed executor should reject a resource-backed managed session");

            for error in [ordinary, resources] {
                assert_eq!(error.kind(), CommandErrorKind::ManagedOperationFailed);
                assert!(matches!(
                    error.reason(),
                    CommandErrorReason::ManagedOperationFailed { diagnostic }
                        if diagnostic.starts_with("managed ") && diagnostic.contains("session admission failed:")
                ));
                assert!(error.output().is_none());
            }
        }

        #[test]
        fn test_scoped_managed_session_methods_report_wrong_manager_rejection() {
            let (manager, _executor, runner) = setup();
            let other_manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
            let wrong_executor =
                CommandExecutor::new(other_manager, CommandExecutionConfig::default()).expect("executor should start");

            let outcome = block_on(
                manager
                    .as_async()
                    .run(OperationOptions::default(), move |scope| async move {
                        let ordinary = runner
                            .start_session_in(
                                &scope,
                                &wrong_executor,
                                Command::new("__must_not_start_for_wrong_session_manager__"),
                                Default::default(),
                            )
                            .await
                            .err()
                            .expect("wrong manager should reject a scoped session");
                        let resources = runner
                            .start_session_resources_in(
                                &scope,
                                &wrong_executor,
                                Command::new("__must_not_start_for_wrong_session_manager__"),
                                CommandResourceRunOptions::new(),
                            )
                            .await
                            .err()
                            .expect("wrong manager should reject a scoped resource session");

                        for error in [ordinary, resources] {
                            assert_eq!(error.kind(), CommandErrorKind::ManagedOperationFailed);
                            assert!(matches!(
                                error.reason(),
                                CommandErrorReason::ManagedOperationFailed { diagnostic }
                                    if diagnostic.starts_with("group binding failed:")
                            ));
                            assert!(error.output().is_none());
                        }
                        Ok::<_, std::convert::Infallible>(())
                    }),
            )
            .expect("parent operation should complete");
            assert!(matches!(outcome, OperationOutcome::Succeeded(())));
        }

        #[test]
        fn test_scoped_async_runner_maps_closed_executor_rejection() {
            let (manager, executor, runner) = setup();
            let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
            let scoped_executor = executor.clone();
            let scoped_runner = runner.clone();

            let outcome = block_on(manager.as_async().run(
                qubit_operation::OperationOptions::default(),
                move |scope| async move {
                    let ordinary = scoped_runner
                        .run_async_in(
                            &scope,
                            &scoped_executor,
                            Command::new("must-not-spawn-after-scoped-async-shutdown"),
                            Default::default(),
                        )
                        .await
                        .expect_err("closed executor should reject a scoped async request");
                    let resources = scoped_runner
                        .run_async_resources_in(
                            &scope,
                            &scoped_executor,
                            Command::new("must-not-spawn-after-scoped-async-shutdown"),
                            CommandResourceRunOptions::new(),
                        )
                        .await
                        .expect_err("closed executor should reject a scoped async resource request");

                    for error in [ordinary, resources] {
                        assert_eq!(error.kind(), crate::CommandErrorKind::ManagedOperationFailed);
                        assert!(matches!(
                            error.reason(),
                            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                                if diagnostic.starts_with("submission rejected:")
                        ));
                        assert!(error.output().is_none());
                    }
                    Ok::<_, std::convert::Infallible>(())
                },
            ))
            .expect("parent operation should complete");
            assert!(matches!(outcome, OperationOutcome::Succeeded(())));
        }

        #[test]
        fn test_async_scoped_runner_maps_group_rejection_for_standard_and_resource_calls() {
            let (manager, _executor, runner) = setup();
            let other_manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
            let wrong_executor =
                CommandExecutor::new(other_manager, CommandExecutionConfig::default()).expect("executor should start");
            let outcome = block_on(
                manager
                    .as_async()
                    .run(OperationOptions::default(), move |scope| async move {
                        let ordinary = runner
                            .run_async_in(
                                &scope,
                                &wrong_executor,
                                Command::new("__must_not_spawn_for_wrong_manager__"),
                                Default::default(),
                            )
                            .await
                            .expect_err("wrong manager should reject a standard scoped request");
                        let resources = runner
                            .run_async_resources_in(
                                &scope,
                                &wrong_executor,
                                Command::new("__must_not_spawn_for_wrong_manager__"),
                                CommandResourceRunOptions::new(),
                            )
                            .await
                            .expect_err("wrong manager should reject a resource scoped request");

                        for error in [ordinary, resources] {
                            assert_eq!(error.kind(), CommandErrorKind::ManagedOperationFailed);
                            assert!(matches!(
                                error.reason(),
                                CommandErrorReason::ManagedOperationFailed { diagnostic }
                                    if diagnostic == "submission rejected: GroupBinding(WrongManager)"
                            ));
                            assert!(error.output().is_none());
                        }
                        Ok::<_, crate::CommandError>(())
                    }),
            )
            .expect("scope should catch both rejected child submissions");
            assert!(matches!(outcome, OperationOutcome::Succeeded(())));
        }

        #[test]
        fn test_async_runner_methods_collect_root_and_scoped_results_and_sessions() {
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
                            Command::new("sh").arg("-c").arg("printf async-scoped-resource"),
                            CommandResourceRunOptions::new(),
                        )
                        .await?;
                    assert_eq!(resources.stdout(), b"async-scoped-resource");
                    let session = scoped_runner
                        .start_session_in(
                            &scope,
                            &scoped_executor,
                            Command::new("sh").arg("-c").arg("printf async-scoped-session"),
                            Default::default(),
                        )
                        .await?;
                    assert_eq!(session_bytes(session).await.0, b"async-scoped-session");
                    let session = scoped_runner
                        .start_session_resources_in(
                            &scope,
                            &scoped_executor,
                            Command::new("sh").arg("-c").arg("printf async-scoped-resource-session"),
                            CommandResourceRunOptions::new(),
                        )
                        .await?;
                    assert_eq!(session_bytes(session).await.0, b"async-scoped-resource-session");
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
            assert!(matches!(error.reason(), crate::CommandErrorReason::SpawnFailed { .. }));
        }

        #[test]
        fn test_collect_async_maps_failed_command_and_preserves_output() {
            let (_manager, executor, runner) = setup();
            let error = block_on(runner.run_async(
                &executor,
                Command::new("sh").arg("-c").arg("printf async-failure; exit 17"),
                Default::default(),
            ))
            .expect_err("a non-success exit status should be returned as a command error");

            assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
            assert!(matches!(
                error.reason(),
                CommandErrorReason::UnexpectedExit {
                    exit_code: Some(17),
                    ..
                }
            ));
            assert_eq!(
                error.captured_output().expect("captured output").stdout(),
                b"async-failure"
            );
        }

        #[test]
        fn test_collect_async_returns_successful_command_output() {
            let (_manager, executor, runner) = setup();
            let output = block_on(runner.run_async(
                &executor,
                Command::new("sh").arg("-c").arg("printf async-success"),
                Default::default(),
            ))
            .expect("a successful command should return its output");

            assert_eq!(output.stdout(), b"async-success");
            assert_eq!(output.stderr(), b"");
            assert_eq!(output.exit_code(), Some(0));
        }

        #[test]
        fn test_collect_async_maps_stopped_operation_and_retains_runner_diagnostic() {
            let (_manager, executor, runner) = setup();
            let directory = tempfile::tempdir().expect("readiness directory should be created");
            let ready = directory.path().join("child-ready");
            let tee = directory.path().join("captured-stdout");
            let script = format!("printf partial; touch {}; exec sleep 30", ready.display());
            let operation = executor
                .submit_async(
                    runner,
                    Command::new("sh").arg("-c").arg(&script),
                    crate::CommandRunOptions::new().tee_stdout_to_file(&tee),
                )
                .expect("async command should be admitted");

            let deadline = Instant::now() + Duration::from_secs(5);
            while (!ready.exists() || std::fs::read(&tee).ok().as_deref() != Some(b"partial"))
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(ready.exists(), "child should signal readiness before stop is requested");
            assert_eq!(std::fs::read(&tee).expect("stdout tee should be readable"), b"partial");
            assert_eq!(
                operation.request_stop(qubit_operation::StopReason::UserCancelled),
                qubit_operation::StopRequestStatus::Accepted
            );

            let error = block_on(collect_async(operation)).expect_err("stopped command should return its diagnostic");
            assert_eq!(error.kind(), CommandErrorKind::Cancelled);
            assert!(matches!(error.reason(), CommandErrorReason::Cancelled));
            assert_eq!(error.stop_reason(), Some(qubit_operation::StopReason::UserCancelled));
            assert_eq!(
                error
                    .captured_output()
                    .expect("partial output should be retained")
                    .stdout(),
                b"partial"
            );
        }
    }
}

#[cfg(test)]
mod outcome_mapping_tests {
    use qubit_operation::ChildFailureSummary;
    use qubit_operation::ChildOutcomeKind;
    use qubit_operation::ChildOutcomeSummary;
    use qubit_operation::DiagnosticSummary;
    use qubit_operation::ExecutionFailure;
    use qubit_operation::ExecutionPhase;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationId;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PanicSummary;
    use qubit_operation::StopReason;
    use qubit_operation::StopReport;

    use super::map_outcome;

    fn operation_id() -> OperationId {
        let manager = OperationManager::new(ManagerOptions::default()).expect("test operation manager should start");
        match manager
            .run(OperationOptions::default(), |scope| {
                Ok::<_, std::convert::Infallible>(scope.id())
            })
            .expect("test operation should complete")
        {
            OperationOutcome::Succeeded(id) => id,
            other => panic!("test operation did not succeed: {other:?}"),
        }
    }

    #[test]
    fn test_map_outcome_preserves_all_terminal_failure_details() {
        type Outcome = OperationOutcome<crate::CommandOutput, crate::CommandError>;
        let id = operation_id();

        let stopped = map_outcome(Outcome::Stopped(StopReport {
            reason: StopReason::UserCancelled,
            accepted_at: None,
        }))
        .expect_err("a stopped operation should map to its command error");
        assert!(matches!(stopped.reason(), crate::CommandErrorReason::Cancelled));
        assert_eq!(stopped.command(), "<managed command>");

        let child_failed = map_outcome(Outcome::ChildFailed(ChildFailureSummary {
            operation_id: id,
            outcome: ChildOutcomeSummary {
                operation_id: id,
                kind: ChildOutcomeKind::Failed,
                summary: DiagnosticSummary::new("required child failed"),
            },
        }))
        .expect_err("a required child failure should become a managed error");
        assert!(matches!(
            child_failed.reason(),
            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                if diagnostic == "required child failed"
        ));

        let panicked = map_outcome(Outcome::Panicked(PanicSummary {
            operation_id: id,
            phase: ExecutionPhase::Running,
            summary: DiagnosticSummary::new("command worker panicked"),
        }))
        .expect_err("a panic summary should become a managed error");
        assert!(matches!(
            panicked.reason(),
            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                if diagnostic == "command worker panicked"
        ));

        let execution_failed = map_outcome(Outcome::ExecutionFailed(ExecutionFailure {
            operation_id: id,
            summary: DiagnosticSummary::new("executor rejected operation body"),
            source: None,
        }))
        .expect_err("an executor failure should become a managed error");
        assert!(matches!(
            execution_failed.reason(),
            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                if diagnostic == "executor rejected operation body"
        ));

        let original = crate::CommandError::from_reason("original command", crate::CommandErrorReason::Cancelled, None);
        let failed = map_outcome(Outcome::Failed(original)).expect_err("an application error should be preserved");
        assert_eq!(failed.command(), "original command");
        assert!(matches!(failed.reason(), crate::CommandErrorReason::Cancelled));
    }
}

#[cfg(all(test, feature = "async", unix))]
mod session_accessor_tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::Duration;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use super::ManagedCommandRunnerApi;
    use crate::Command;
    use crate::CommandExecutionConfig;
    use crate::CommandExecutor;
    use crate::CommandRunner;
    use crate::session::CommandSessionApi;

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
            std::thread::park_timeout(Duration::from_millis(100));
        }
    }

    #[test]
    fn test_session_operation_accessor_borrows_then_transfers_handle() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).expect("executor should start");
        let runner = CommandRunner::without_timeout();
        let mut session =
            block_on(runner.start_session(&executor, Command::new("sh").arg("-c").arg(":"), Default::default()))
                .expect("session should start");

        assert!(
            session.operation().is_some(),
            "session should expose its operation by borrow"
        );
        drop(session.take_stdin().expect("stdin endpoint should be available"));
        assert!(
            session.take_stdin().is_none(),
            "stdin ownership can only be transferred once"
        );
        drop(session.take_stdout().expect("stdout endpoint should be available"));
        assert!(
            session.take_stdout().is_none(),
            "stdout ownership can only be transferred once"
        );
        drop(session.take_stderr().expect("stderr endpoint should be available"));
        assert!(
            session.take_stderr().is_none(),
            "stderr ownership can only be transferred once"
        );
        let mut operation = session
            .take_operation()
            .expect("session should transfer its operation handle");
        assert!(
            session.operation().is_none(),
            "taking the handle should clear the borrowed slot"
        );
        assert!(
            session.take_operation().is_none(),
            "operation ownership can only be transferred once"
        );
        assert!(matches!(
            operation.wait_outcome().expect("session outcome should be available"),
            OperationOutcome::Succeeded(_)
        ));
        operation.wait_settlement().expect("session operation should settle");
    }
}

#[cfg(test)]
mod outcome_mapping_coverage_tests {
    use qubit_operation::ChildFailureSummary;
    use qubit_operation::ChildOutcomeKind;
    use qubit_operation::ChildOutcomeSummary;
    use qubit_operation::DiagnosticSummary;
    use qubit_operation::ExecutionFailure;
    use qubit_operation::ExecutionPhase;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PanicSummary;
    use qubit_operation::StopReason;
    use qubit_operation::StopReport;

    use super::map_outcome;
    use super::map_outcome_with_stop_diagnostic;
    use crate::Command;
    use crate::CommandError;
    use crate::CommandErrorReason;
    use crate::CommandExecutionConfig;
    use crate::CommandExecutor;
    use crate::CommandRunner;
    use crate::command_runner::ManagedCommandRunnerApi;

    fn operation_id() -> qubit_operation::OperationId {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let id = handle.id();
        let _ = handle.wait_outcome().unwrap();
        handle.wait_settlement().unwrap();
        id
    }

    #[test]
    fn test_user_cancellation_keeps_legacy_error_category() {
        let actual = map_outcome(OperationOutcome::<crate::CommandOutput, CommandError>::Stopped(
            StopReport {
                reason: StopReason::UserCancelled,
                accepted_at: None,
            },
        ))
        .unwrap_err();

        assert_eq!(actual.command(), "<managed command>");
        assert_eq!(actual.stop_reason(), Some(StopReason::UserCancelled));
        assert!(matches!(actual.reason(), CommandErrorReason::Cancelled));
    }

    #[test]
    fn test_stopped_timeout_keeps_timeout_reason() {
        for reason in [
            StopReason::Timeout,
            StopReason::ParentStopped,
            StopReason::ManagerShutdown,
            StopReason::RequiredChildFailed,
        ] {
            let actual = map_outcome(OperationOutcome::<crate::CommandOutput, CommandError>::Stopped(
                StopReport {
                    reason,
                    accepted_at: None,
                },
            ))
            .unwrap_err();

            assert!(matches!(actual.reason(), CommandErrorReason::Stopped { reason: actual } if *actual == reason));
        }
    }

    #[test]
    fn test_managed_stop_mapping_preserves_runner_output_and_stop_cause() {
        let diagnostic =
            CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_captured_output(
                crate::CommandCapturedOutput::new((b"partial".to_vec(), false, true), (b"tail".to_vec(), false, true)),
            );
        let actual = map_outcome_with_stop_diagnostic(
            OperationOutcome::Stopped(StopReport {
                reason: StopReason::ManagerShutdown,
                accepted_at: None,
            }),
            Some(diagnostic),
        )
        .unwrap_err();

        assert!(matches!(
            actual.reason(),
            CommandErrorReason::Stopped {
                reason: StopReason::ManagerShutdown
            }
        ));
        assert_eq!(actual.stop_reason(), Some(StopReason::ManagerShutdown));
        let captured = actual.captured_output().expect("captured bytes should survive");
        assert_eq!(captured.stdout(), b"partial");
        assert_eq!(captured.stderr(), b"tail");
    }

    #[test]
    fn test_child_panic_and_execution_failures_keep_diagnostics() {
        let id = operation_id();
        let child = ChildFailureSummary {
            operation_id: id,
            outcome: ChildOutcomeSummary {
                operation_id: id,
                kind: ChildOutcomeKind::Failed,
                summary: DiagnosticSummary::new("required child failed"),
            },
        };
        let child_error = map_outcome(OperationOutcome::<crate::CommandOutput, CommandError>::ChildFailed(
            child,
        ))
        .unwrap_err();
        assert!(matches!(
            child_error.reason(),
            CommandErrorReason::ManagedOperationFailed { diagnostic }
                if diagnostic == "required child failed"
        ));

        let panic_error = map_outcome(OperationOutcome::<crate::CommandOutput, CommandError>::Panicked(
            PanicSummary {
                operation_id: id,
                phase: ExecutionPhase::Running,
                summary: DiagnosticSummary::new("managed worker panicked"),
            },
        ))
        .unwrap_err();
        assert!(matches!(
            panic_error.reason(),
            CommandErrorReason::ManagedOperationFailed { diagnostic }
                if diagnostic == "managed worker panicked"
        ));

        let execution_error = map_outcome(OperationOutcome::<crate::CommandOutput, CommandError>::ExecutionFailed(
            ExecutionFailure {
                operation_id: id,
                summary: DiagnosticSummary::new("executor could not run command"),
                source: None,
            },
        ))
        .unwrap_err();
        assert!(matches!(
            execution_error.reason(),
            CommandErrorReason::ManagedOperationFailed { diagnostic }
                if diagnostic == "executor could not run command"
        ));
    }

    #[test]
    fn test_managed_runner_keeps_spawn_failure() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
        let missing_command = "__qubit_command_missing_managed_api_test_binary__";

        let error = CommandRunner::without_timeout()
            .run_managed(&executor, Command::new(missing_command), Default::default())
            .expect_err("a missing executable should remain a command failure");

        assert!(error.command().contains(missing_command));
        assert!(error.output().is_none());
        assert!(matches!(error.reason(), CommandErrorReason::SpawnFailed { .. }));
    }
}
