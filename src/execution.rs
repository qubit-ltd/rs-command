//! Managed command execution contracts and bounded configuration.
use crate::{Command, CommandError, CommandResourceRunOptions, CommandRunOptions, CommandRunner};
#[cfg(feature = "async")]
use qubit_operation::AsyncOperationHandle;
use qubit_operation::OperationHandle;
use thiserror::Error;
#[path = "execution/adapter/mod.rs"]
pub mod adapter;
#[path = "execution/backend/mod.rs"]
pub mod backend;
#[path = "execution/contracts/mod.rs"]
pub mod contracts;
#[path = "execution/files/mod.rs"]
pub mod files;
#[path = "execution/scheduler/mod.rs"]
pub mod scheduler;
pub use contracts::{
    ActivatedRequest, Backend, CapacityFailure, EnqueueFailure, FactSink, FilePreparationFailure,
    PreparedFiles, RecoveryOwner, RecoveryPublishError, RecoveryPublishFailure, RecoveryRetryError,
    Request, Reservation, ResourceInput, ScheduledJob, Scheduler, StopPort,
};

/// Synchronous command operation handle.
pub type CommandOperation = OperationHandle<crate::CommandOutput, CommandError>;
/// Asynchronous command operation handle.
#[cfg(feature = "async")]
pub type AsyncCommandOperation = AsyncOperationHandle<crate::CommandOutput, CommandError>;

/// Limits retained by a command executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandExecutionConfig {
    pub max_active: usize,
    pub max_queued: usize,
    pub max_waiters: usize,
    pub max_queued_input_bytes: usize,
    pub max_diagnostics: usize,
}
impl Default for CommandExecutionConfig {
    fn default() -> Self {
        Self {
            max_active: 4,
            max_queued: 64,
            max_waiters: 64,
            max_queued_input_bytes: 16 * 1024 * 1024,
            max_diagnostics: 64,
        }
    }
}
impl CommandExecutionConfig {
    pub fn validate(&self) -> Result<(), CommandExecutorCreateError> {
        if self.max_active == 0 {
            return Err(CommandExecutorCreateError::ZeroActiveCapacity);
        }
        Ok(())
    }
}
/// Invalid executor configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum CommandExecutorCreateError {
    #[error("max_active must be greater than zero")]
    ZeroActiveCapacity,
    #[error("failed to start managed command worker")]
    WorkerStartFailed,
    #[error("failed to bind command service operation group: {0}")]
    GroupBinding(qubit_operation::GroupBindingError),
}
/// Executor configuration bound to an operation manager.
#[derive(Clone)]
pub struct CommandExecutor {
    manager: qubit_operation::OperationManager,
    config: CommandExecutionConfig,
    pub(crate) scheduler: std::sync::Arc<scheduler::BoundedScheduler>,
    pub(crate) backend: std::sync::Arc<backend::ProcessBackend>,
    pub(crate) group: qubit_operation::OperationGroup,
    pub(crate) recovery: std::sync::mpsc::SyncSender<adapter::cleanup::RecoveryItem>,
}
impl CommandExecutor {
    pub fn new(
        manager: qubit_operation::OperationManager,
        config: CommandExecutionConfig,
    ) -> Result<Self, CommandExecutorCreateError> {
        config.validate()?;
        let (scheduler, backend, group, recovery, manager) =
            executor::build_executor(manager.clone(), config.clone())?;
        Ok(Self {
            manager,
            config,
            scheduler: std::sync::Arc::new(scheduler),
            backend: std::sync::Arc::new(backend),
            group,
            recovery,
        })
    }
    /// Returns the immutable executor limits.
    pub fn config(&self) -> &CommandExecutionConfig {
        &self.config
    }
    /// Returns the manager view used for future submissions.
    pub fn manager(&self) -> &qubit_operation::OperationManager {
        &self.manager
    }
}
#[path = "execution/executor.rs"]
mod executor;
/// Frozen submission signatures implemented by the managed executor integration.
pub trait CommandExecutorApi {
    fn submit(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    fn submit_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    fn submit_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    fn submit_resources_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    fn submit_wait_blocking_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    fn submit_wait_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    fn submit_wait_resources_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    fn submit_wait_resources_blocking_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    #[cfg(feature = "async")]
    fn submit_wait(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandSubmitFailure>> + Send;
    #[cfg(feature = "async")]
    fn submit_wait_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<
        Output = Result<AsyncCommandOperation, CommandResourceSubmitFailure>,
    > + Send;
    #[cfg(feature = "async")]
    fn submit_wait_resources_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<
        Output = Result<AsyncCommandOperation, CommandResourceSubmitFailure>,
    > + Send
    + 'a;
    #[cfg(feature = "async")]
    fn submit_async(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure>;
    #[cfg(feature = "async")]
    fn submit_async_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure>;
    #[cfg(feature = "async")]
    fn submit_async_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure>;
    #[cfg(feature = "async")]
    fn submit_async_resources_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure>;
    #[cfg(feature = "async")]
    fn submit_wait_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandSubmitFailure>> + Send + 'a;
}

/// Inputs returned when ordinary submission is rejected before ownership transfers.
#[derive(Debug)]
pub struct CommandSubmitFailure {
    pub runner: CommandRunner,
    pub command: Command,
    pub options: CommandRunOptions,
    pub reason: CommandSubmitFailureReason,
}
/// Input rejection reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandSubmitFailureReason {
    Capacity(CapacityFailure),
    Admission(qubit_operation::AdmissionError),
    GroupBinding(qubit_operation::GroupBindingError),
    Snapshot(SnapshotFailure),
}
/// Concrete diagnostic retained when submission snapshots cannot be captured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotFailure {
    pub diagnostic: String,
}
/// Inputs returned when resource submission is rejected.
#[derive(Debug)]
pub struct CommandResourceSubmitFailure {
    pub runner: CommandRunner,
    pub command: Command,
    pub options: CommandResourceRunOptions,
    pub reason: CommandSubmitFailureReason,
}
