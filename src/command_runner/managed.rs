//! Managed command runner entry point namespace.

use crate::{
    Command, CommandError, CommandResourceRunOptions, CommandRunOptions, CommandRunner,
    CommandSession,
};
use qubit_operation::OperationScope;

/// Frozen managed command runner entry points; implementation is owned by T6.
pub trait ManagedCommandRunnerApi {
    fn run_managed_in(
        &self,
        scope: &OperationScope,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::CommandOperation, crate::CommandSubmitFailure>;
    fn run_resources_managed_in(
        &self,
        scope: &OperationScope,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOperation, crate::CommandResourceSubmitFailure>;
    #[cfg(feature = "async")]
    fn run_async_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<
        Output = Result<crate::AsyncCommandOperation, crate::CommandSubmitFailure>,
    > + Send
    + 'a;
    fn start_session_in(
        &self,
        scope: &OperationScope,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandSession, CommandError>;
}

// Keep imported names in the frozen API module's scope for IDE documentation.
const _: Option<fn(&CommandRunner)> = None;
