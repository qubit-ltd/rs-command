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

/// Submission entry points for [`CommandExecutor`](super::CommandExecutor).
///
/// Immediate methods attempt local admission; waiting methods retry only capacity-full and
/// manager-busy refusals while retaining submitted inputs. Scoped variants use the operation
/// scope's group and context. Resource variants transfer owned files after successful admission.
/// Returned operation outcomes and resource settlement are separate observations.
pub trait CommandExecutorApi {
    /// Attempts to submit a command without waiting for executor capacity. Use the waiting submission
    /// methods when the caller intends to wait for local admission capacity. The returned operation has
    /// a one-shot outcome and a separate settlement phase; observing the outcome alone does not prove
    /// process and I/O resources are released.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. The failure carries the runner, command,
    /// and options.
    fn submit(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Attempts to submit a command without waiting for executor capacity. Use the waiting submission
    /// methods when the caller intends to wait for local admission capacity. The returned operation has
    /// a one-shot outcome and a separate settlement phase; observing the outcome alone does not prove
    /// process and I/O resources are released. On success, ownership of supplied stdin, stdout, and
    /// stderr files transfers to the operation. Reservation precedes worker-side preparation, which may
    /// truncate outputs. Rejected admission returns the handles; later preparation or execution
    /// failures use operation outcome and recovery paths and do not promise unchanged files.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. The failure carries the runner and command; its options retain the
    /// owned file handles on rejected admission.
    fn submit_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Attempts to submit a command without waiting for executor capacity. Use the waiting submission
    /// methods when the caller intends to wait for local admission capacity. The returned operation has
    /// a one-shot outcome and a separate settlement phase; observing the outcome alone does not prove
    /// process and I/O resources are released. The scope binds the operation to this executor group and
    /// supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. Scoped variants can also fail to bind the
    /// operation group or reach the scope deadline. The failure carries the runner, command, and
    /// options.
    fn submit_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Attempts to submit a command without waiting for executor capacity. Use the waiting submission
    /// methods when the caller intends to wait for local admission capacity. The returned operation has
    /// a one-shot outcome and a separate settlement phase; observing the outcome alone does not prove
    /// process and I/O resources are released. On success, ownership of supplied stdin, stdout, and
    /// stderr files transfers to the operation. Reservation precedes worker-side preparation, which may
    /// truncate outputs. Rejected admission returns the handles; later preparation or execution
    /// failures use operation outcome and recovery paths and do not promise unchanged files. The scope
    /// binds the operation to this executor group and supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. Scoped variants can also fail to bind the operation group or reach
    /// the scope deadline. The failure carries the runner and command; its options retain the owned
    /// file handles on rejected admission.
    fn submit_resources_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Blocks the calling thread while waiting for local executor admission capacity, then returns the
    /// submitted operation. The wait retains runner, command, and options while retrying capacity-full
    /// or manager-busy refusals. Waiter limits, executor shutdown, scope cancellation, and the
    /// applicable deadline can end the wait. A scoped retry uses the original scope context and does
    /// not extend its deadline. The returned operation has a one-shot outcome and a separate settlement
    /// phase; observing the outcome alone does not prove process and I/O resources are released. The
    /// scope binds the operation to this executor group and supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. Scoped variants can also fail to bind the
    /// operation group or reach the scope deadline. A waiting variant can also fail when waiter
    /// registration is full, the scope is stopped, or its deadline expires. The failure carries the
    /// runner, command, and options.
    fn submit_wait_blocking_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Blocks the calling thread while waiting for local executor admission capacity, then returns the
    /// submitted operation. The wait retains runner, command, and options while retrying capacity-full
    /// or manager-busy refusals. Waiter limits, executor shutdown, scope cancellation, and the
    /// applicable deadline can end the wait. The unscoped form has no operation-scope deadline. The
    /// returned operation has a one-shot outcome and a separate settlement phase; observing the outcome
    /// alone does not prove process and I/O resources are released.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. A waiting variant can also fail when waiter
    /// registration is full. The failure carries the runner, command, and options.
    fn submit_wait_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure>;
    /// Blocks the calling thread while waiting for local executor admission capacity, then returns the
    /// submitted operation. The wait retains runner, command, and options while retrying capacity-full
    /// or manager-busy refusals. Waiter limits, executor shutdown, scope cancellation, and the
    /// applicable deadline can end the wait. The unscoped form has no operation-scope deadline. The
    /// returned operation has a one-shot outcome and a separate settlement phase; observing the outcome
    /// alone does not prove process and I/O resources are released. On success, ownership of supplied
    /// stdin, stdout, and stderr files transfers to the operation. Reservation precedes worker-side
    /// preparation, which may truncate outputs. Rejected admission returns the handles; later
    /// preparation or execution failures use operation outcome and recovery paths and do not promise
    /// unchanged files.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. A waiting variant can also fail when waiter registration is full.
    /// The failure carries the runner and command; its options retain the owned file handles on
    /// rejected admission. Requests over budget are rejected immediately rather than retried.
    fn submit_wait_resources_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Blocks the calling thread while waiting for local executor admission capacity, then returns the
    /// submitted operation. The wait retains runner, command, and options while retrying capacity-full
    /// or manager-busy refusals. Waiter limits, executor shutdown, scope cancellation, and the
    /// applicable deadline can end the wait. A scoped retry uses the original scope context and does
    /// not extend its deadline. The returned operation has a one-shot outcome and a separate settlement
    /// phase; observing the outcome alone does not prove process and I/O resources are released. On
    /// success, ownership of supplied stdin, stdout, and stderr files transfers to the operation.
    /// Reservation precedes worker-side preparation, which may truncate outputs. Rejected admission
    /// returns the handles; later preparation or execution failures use operation outcome and recovery
    /// paths and do not promise unchanged files. The scope binds the operation to this executor group
    /// and supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. Scoped variants can also fail to bind the operation group or reach
    /// the scope deadline. A waiting variant can also fail when waiter registration is full, the scope
    /// is stopped, or its deadline expires. The failure carries the runner and command; its options
    /// retain the owned file handles on rejected admission. Requests over budget are rejected
    /// immediately rather than retried.
    fn submit_wait_resources_blocking_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure>;
    /// Asynchronously waits without blocking the caller thread for local executor admission capacity,
    /// then returns the submitted operation. The wait retains runner, command, and options while
    /// retrying capacity-full or manager-busy refusals. Waiter limits, executor shutdown, scope
    /// cancellation, and the applicable deadline can end the wait. The unscoped form has no operation-
    /// scope deadline. The returned operation has a one-shot outcome and a separate settlement phase;
    /// observing the outcome alone does not prove process and I/O resources are released.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. A waiting variant can also fail when waiter
    /// registration is full. The failure carries the runner, command, and options.
    #[cfg(feature = "async")]
    fn submit_wait(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandSubmitFailure>> + Send;
    /// Asynchronously waits without blocking the caller thread for local executor admission capacity,
    /// then returns the submitted operation. The wait retains runner, command, and options while
    /// retrying capacity-full or manager-busy refusals. Waiter limits, executor shutdown, scope
    /// cancellation, and the applicable deadline can end the wait. The unscoped form has no operation-
    /// scope deadline. The returned operation has a one-shot outcome and a separate settlement phase;
    /// observing the outcome alone does not prove process and I/O resources are released. On success,
    /// ownership of supplied stdin, stdout, and stderr files transfers to the operation. Reservation
    /// precedes worker-side preparation, which may truncate outputs. Rejected admission returns the
    /// handles; later preparation or execution failures use operation outcome and recovery paths and do
    /// not promise unchanged files.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. A waiting variant can also fail when waiter registration is full.
    /// The failure carries the runner and command; its options retain the owned file handles on
    /// rejected admission. Requests over budget are rejected immediately rather than retried.
    #[cfg(feature = "async")]
    fn submit_wait_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandResourceSubmitFailure>> + Send;
    /// Asynchronously waits without blocking the caller thread for local executor admission capacity,
    /// then returns the submitted operation. The wait retains runner, command, and options while
    /// retrying capacity-full or manager-busy refusals. Waiter limits, executor shutdown, scope
    /// cancellation, and the applicable deadline can end the wait. A scoped retry uses the original
    /// scope context and does not extend its deadline. The returned operation has a one-shot outcome
    /// and a separate settlement phase; observing the outcome alone does not prove process and I/O
    /// resources are released. On success, ownership of supplied stdin, stdout, and stderr files
    /// transfers to the operation. Reservation precedes worker-side preparation, which may truncate
    /// outputs. Rejected admission returns the handles; later preparation or execution failures use
    /// operation outcome and recovery paths and do not promise unchanged files. The scope binds the
    /// operation to this executor group and supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. Scoped variants can also fail to bind the operation group or reach
    /// the scope deadline. A waiting variant can also fail when waiter registration is full, the scope
    /// is stopped, or its deadline expires. The failure carries the runner and command; its options
    /// retain the owned file handles on rejected admission. Requests over budget are rejected
    /// immediately rather than retried.
    #[cfg(feature = "async")]
    fn submit_wait_resources_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandResourceSubmitFailure>> + Send + 'a;
    /// Attempts local admission and returns an asynchronous view of the submitted operation without
    /// waiting for executor capacity. Use the async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement phase; observing the
    /// outcome alone does not prove process and I/O resources are released.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. The failure carries the runner, command,
    /// and options.
    #[cfg(feature = "async")]
    fn submit_async(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure>;
    /// Attempts local admission and returns an asynchronous view of the submitted operation without
    /// waiting for executor capacity. Use the async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement phase; observing the
    /// outcome alone does not prove process and I/O resources are released. On success, ownership of
    /// supplied stdin, stdout, and stderr files transfers to the operation. Reservation precedes
    /// worker-side preparation, which may truncate outputs. Rejected admission returns the handles;
    /// later preparation or execution failures use operation outcome and recovery paths and do not
    /// promise unchanged files.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. The failure carries the runner and command; its options retain the
    /// owned file handles on rejected admission.
    #[cfg(feature = "async")]
    fn submit_async_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure>;
    /// Attempts local admission and returns an asynchronous view of the submitted operation without
    /// waiting for executor capacity. Use the async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement phase; observing the
    /// outcome alone does not prove process and I/O resources are released. The scope binds the
    /// operation to this executor group and supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. Scoped variants can also fail to bind the
    /// operation group or reach the scope deadline. The failure carries the runner, command, and
    /// options.
    #[cfg(feature = "async")]
    fn submit_async_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure>;
    /// Attempts local admission and returns an asynchronous view of the submitted operation without
    /// waiting for executor capacity. Use the async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement phase; observing the
    /// outcome alone does not prove process and I/O resources are released. On success, ownership of
    /// supplied stdin, stdout, and stderr files transfers to the operation. Reservation precedes
    /// worker-side preparation, which may truncate outputs. Rejected admission returns the handles;
    /// later preparation or execution failures use operation outcome and recovery paths and do not
    /// promise unchanged files. The scope binds the operation to this executor group and supplies its
    /// context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation fails, the estimated
    /// file bytes exceed the configured budget, executor capacity is full or closed, or operation-
    /// manager admission is denied. Scoped variants can also fail to bind the operation group or reach
    /// the scope deadline. The failure carries the runner and command; its options retain the owned
    /// file handles on rejected admission.
    #[cfg(feature = "async")]
    fn submit_async_resources_in(
        &self,
        scope: &qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure>;
    /// Asynchronously waits without blocking the caller thread for local executor admission capacity,
    /// then returns the submitted operation. The wait retains runner, command, and options while
    /// retrying capacity-full or manager-busy refusals. Waiter limits, executor shutdown, scope
    /// cancellation, and the applicable deadline can end the wait. A scoped retry uses the original
    /// scope context and does not extend its deadline. The returned operation has a one-shot outcome
    /// and a separate settlement phase; observing the outcome alone does not prove process and I/O
    /// resources are released. The scope binds the operation to this executor group and supplies its
    /// context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails, executor capacity is full
    /// or closed, or operation-manager admission is denied. Scoped variants can also fail to bind the
    /// operation group or reach the scope deadline. A waiting variant can also fail when waiter
    /// registration is full, the scope is stopped, or its deadline expires. The failure carries the
    /// runner, command, and options.
    #[cfg(feature = "async")]
    fn submit_wait_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> impl std::future::Future<Output = Result<AsyncCommandOperation, CommandSubmitFailure>> + Send + 'a;
}
