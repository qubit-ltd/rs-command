//! Frozen managed command runner entry points.
#[cfg(feature = "async")]
use crate::CommandSession;
use crate::{Command, CommandError, CommandExecutor, CommandResourceRunOptions, CommandRunOptions};
#[cfg(feature = "async")]
use crate::{CommandStderr, CommandStdin, CommandStdout};
use qubit_operation::OperationScope;

/// Managed runner signatures; behavior is implemented by T6.
pub trait ManagedCommandRunnerApi {
    fn run_managed(
        &self,
        executor: &CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    fn run_managed_resources(
        &self,
        executor: &CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    fn run_managed_in(
        &self,
        scope: &OperationScope,
        executor: &CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    fn run_managed_resources_in(
        &self,
        scope: &OperationScope,
        executor: &CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError>;

    #[cfg(feature = "async")]
    fn run_async<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    #[cfg(feature = "async")]
    fn run_async_resources<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    #[cfg(feature = "async")]
    fn start_session<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<CommandSession, CommandError>> + Send + 'a;

    #[cfg(feature = "async")]
    fn start_session_resources<'a>(
        &'a self,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<CommandSession, CommandError>> + Send + 'a;

    #[cfg(feature = "async")]
    fn run_async_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    #[cfg(feature = "async")]
    fn run_async_resources_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<crate::CommandOutput, CommandError>> + Send + 'a;

    #[cfg(feature = "async")]
    fn start_session_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        executor: &'a CommandExecutor,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<CommandSession, CommandError>> + Send + 'a;

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
    #[cfg(feature = "async")]
    async fn run_async<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            e.submit_async(self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
    #[cfg(feature = "async")]
    async fn run_async_resources<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            e.submit_async_resources(self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
    #[cfg(feature = "async")]
    async fn start_session<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<CommandSession, CommandError> {
        e.start_session_managed(self.clone(), c, o)
    }
    #[cfg(feature = "async")]
    async fn start_session_resources<'a>(
        &'a self,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<CommandSession, CommandError> {
        e.start_session_resources_managed(self.clone(), c, o)
    }
    #[cfg(feature = "async")]
    async fn run_async_in<'a>(
        &'a self,
        s: &'a OperationScope,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            e.submit_async_in(s, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
    #[cfg(feature = "async")]
    async fn run_async_resources_in<'a>(
        &'a self,
        s: &'a OperationScope,
        e: &'a CommandExecutor,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<crate::CommandOutput, CommandError> {
        collect_async(
            e.submit_async_resources_in(s, self.clone(), c, o)
                .map_err(|f| managed_error(format!("submission rejected: {:?}", f.reason)))?,
        )
        .await
    }
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
    fn operation(&self) -> Option<&crate::CommandOperation> {
        self.operation.as_ref()
    }
    fn take_operation(&mut self) -> Option<crate::CommandOperation> {
        self.operation.take()
    }
    fn take_stdin(&mut self) -> Option<CommandStdin> {
        self.stdin.take()
    }
    fn take_stdout(&mut self) -> Option<CommandStdout> {
        self.stdout.take()
    }
    fn take_stderr(&mut self) -> Option<CommandStderr> {
        self.stderr.take()
    }
}

fn collect(mut handle: crate::CommandOperation) -> Result<crate::CommandOutput, CommandError> {
    let outcome = handle
        .wait_outcome()
        .map_err(|e| managed_error(format!("operation outcome unavailable: {e}")))?;
    let _settlement = handle
        .wait_settlement()
        .map_err(|e| managed_error(format!("operation settlement unavailable: {e}")))?;
    map_outcome(outcome)
}
#[cfg(feature = "async")]
async fn collect_async(
    mut handle: crate::AsyncCommandOperation,
) -> Result<crate::CommandOutput, CommandError> {
    let outcome = handle
        .wait_outcome()
        .await
        .map_err(|e| managed_error(format!("operation outcome unavailable: {e}")))?;
    let _settlement = handle.wait_settlement().await;
    map_outcome(outcome)
}
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
        qubit_operation::OperationOutcome::Panicked(panic) => {
            Err(managed_error(panic.summary.as_str().to_owned()))
        }
        qubit_operation::OperationOutcome::ExecutionFailed(failure) => {
            Err(managed_error(failure.summary.as_str().to_owned()))
        }
    }
}
fn managed_error(message: String) -> CommandError {
    CommandError::from_reason(
        "<managed command>",
        crate::CommandErrorReason::ManagedOperationFailed {
            diagnostic: message,
        },
        None,
    )
}
