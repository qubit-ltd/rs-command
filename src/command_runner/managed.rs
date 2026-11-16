//! Frozen managed command runner entry points.
#[cfg(feature = "async")]
use crate::CommandSession;
use crate::{Command, CommandError, CommandExecutor, CommandResourceRunOptions, CommandRunOptions};
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
