// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "async")]
use super::AsyncCommandOperation;
use super::CommandOperation;
use super::CommandResourceSubmitFailure;
use super::CommandSubmitFailure;
use crate::Command;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
use crate::CommandRunner;

/// Frozen submission signatures implemented by the managed executor
/// integration.
pub trait CommandExecutorApi {
    /// Performs the submit step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Performs the submit resources step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Performs the submit in step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Performs the submit resources in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit_resources_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Performs the submit wait blocking in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit_wait_blocking_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Performs the submit wait blocking step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit_wait_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Performs the submit wait resources blocking step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit_wait_resources_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Performs the submit wait resources blocking in step in the managed
    /// command execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    fn submit_wait_resources_blocking_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Performs the submit wait step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    fn submit_wait(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandSubmitFailure>> + Send;
    /// Performs the submit wait resources step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    fn submit_wait_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<
        Output = Result<AsyncCommandOperation, CommandResourceSubmitFailure>,
    > + Send;
    /// Performs the submit wait resources in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
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
    /// Performs the submit async step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure>;
    /// Performs the submit async resources step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure>;
    /// Performs the submit async in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure>;
    /// Performs the submit async resources in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async_resources_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure>;
    /// Performs the submit wait in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    fn submit_wait_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandSubmitFailure>> + Send + 'a;
}
