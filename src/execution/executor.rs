// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "async")]
use std::future::Future;
use std::sync::Arc;

use qubit_operation::GroupOptions;
use qubit_operation::OperationContext;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationScope;

use super::CommandExecutionConfig;
use super::CommandExecutor;
use super::CommandExecutorApi;
use super::CommandExecutorCreateError;
use super::CommandOperation;
use super::CommandResourceSubmitFailure;
use super::CommandSubmitFailure;
use super::CommandSubmitFailureReason;
#[cfg(feature = "async")]
use super::RetryTimerFailure;
use super::SnapshotFailure;
use super::adapter::AdapterState;
use super::backend::ProcessBackend;
#[cfg(feature = "async")]
use super::command_executor::RetryDelay;
#[cfg(feature = "async")]
use super::command_executor::internal::async_submission_context::AsyncSubmissionContext;
#[cfg(feature = "async")]
use super::command_executor::internal::capacity_wait::CapacityWait;
#[cfg(feature = "async")]
use super::command_executor::internal::scheduler_waiter_guard::SchedulerWaiterGuard;
#[cfg(test)]
use super::command_executor::wait::deadline_failure;
#[cfg(test)]
use super::command_executor::wait::retry_wait;
#[cfg(all(test, feature = "async"))]
use super::command_executor::wait::retryable_async;
#[cfg(any(feature = "async", all(test, unix)))]
use super::contracts::CapacityFailure;
use super::contracts::ExecutionSnapshot;
use super::contracts::IoPathSnapshot;
use super::contracts::Request;
use super::contracts::RequestOptions;
use super::contracts::Scheduler;
use super::scheduler::BoundedScheduler;
use crate::Command;
#[cfg(feature = "async")]
use crate::CommandError;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
use crate::CommandRunner;

/// Stores the session channels value used by this execution component.
///
/// # Parameters
/// * `runner`: Supplies the runner value used by this operation.
#[cfg(feature = "async")]
fn session_channels(runner: &CommandRunner) -> crate::session::channel::SessionChannels {
    match runner.configured_max_total_output_bytes() {
        Some(limit) => crate::session::channel::SessionChannels::with_policy(
            crate::session::contracts::SessionStreamPolicy::TerminateAfterBytes(limit),
        ),
        None => crate::session::channel::SessionChannels::new(),
    }
}

impl CommandExecutor {
    /// Attempts to submit a command without waiting for executor capacity. Use
    /// the waiting submission methods when the caller intends to wait for
    /// local admission capacity. The returned operation has a one-shot
    /// outcome and a separate settlement phase; observing the outcome alone
    /// does not prove process and I/O resources are released.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails,
    /// executor capacity is full or closed, or manager admission
    /// is denied. The failure carries the runner, command, and options.
    pub fn submit(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        self.submit_in_manager(&self.manager, runner, command, options, None)
    }
    /// Attempts to submit a command without waiting for executor capacity. Use
    /// the waiting submission methods when the caller intends to wait for
    /// local admission capacity. The returned operation has a one-shot
    /// outcome and a separate settlement phase; observing the outcome alone
    /// does not prove process and I/O resources are released. On success,
    /// ownership of supplied stdin, stdout, and stderr files transfers to
    /// the operation. Reservation precedes worker-side preparation, which may
    /// truncate outputs. Rejected admission returns the handles; later
    /// preparation or execution failures use operation outcome and recovery
    /// paths and do not promise unchanged files.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation
    /// fails, the estimated file bytes exceed the configured budget,
    /// executor capacity is full or closed, or manager admission
    /// is denied. The failure carries the runner and command; its options
    /// retain the owned file handles on rejected admission.
    pub fn submit_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let bytes = resource_bytes(&options);
        self.submit_resources_manager(&self.manager, runner, command, options, bytes, None)
    }
    /// Attempts local admission and returns an asynchronous view of the
    /// submitted operation without waiting for executor capacity. Use the
    /// async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement
    /// phase; observing the outcome alone does not prove process and I/O
    /// resources are released.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails,
    /// executor capacity is full or closed, or manager admission
    /// is denied. The failure carries the runner, command, and options.
    #[cfg(feature = "async")]
    pub fn submit_async(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        self.submit_async_in_manager(
            &self.manager,
            runner,
            command,
            RequestOptions::Standard(options),
            AsyncSubmissionContext::plain(None),
        )
    }
    /// Attempts local admission and returns an asynchronous view of the
    /// submitted operation without waiting for executor capacity. Use the
    /// async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement
    /// phase; observing the outcome alone does not prove process and I/O
    /// resources are released. On success, ownership of supplied stdin,
    /// stdout, and stderr files transfers to the operation. Reservation
    /// precedes worker-side preparation, which may truncate outputs.
    /// Rejected admission returns the handles; later preparation or
    /// execution failures use operation outcome and recovery paths and do not
    /// promise unchanged files.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation
    /// fails, the estimated file bytes exceed the configured budget,
    /// executor capacity is full or closed, or manager admission
    /// is denied. The failure carries the runner and command; its options
    /// retain the owned file handles on rejected admission.
    #[cfg(feature = "async")]
    pub fn submit_async_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let bytes = resource_bytes(&options);
        self.submit_async_resources_manager(
            &self.manager,
            runner,
            command,
            options,
            bytes,
            AsyncSubmissionContext::plain(None),
        )
    }
    /// Attempts local admission and returns an asynchronous view of the
    /// submitted operation without waiting for executor capacity. Use the
    /// async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement
    /// phase; observing the outcome alone does not prove process and I/O
    /// resources are released. The scope binds the operation to this
    /// executor group and supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and
    ///   deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails,
    /// executor capacity is full or closed, or manager admission
    /// is denied. Scoped variants can also fail to bind the operation group
    /// or reach the scope deadline. The failure carries the runner, command,
    /// and options.
    #[cfg(feature = "async")]
    pub fn submit_async_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_async_in_manager(
            &manager,
            runner,
            command,
            RequestOptions::Standard(options),
            AsyncSubmissionContext::plain(Some(scope.context())),
        )
    }
    /// Attempts local admission and returns an asynchronous view of the
    /// submitted operation without waiting for executor capacity. Use the
    /// async waiting methods when capacity waiting is required.
    /// The returned operation has a one-shot outcome and a separate settlement
    /// phase; observing the outcome alone does not prove process and I/O
    /// resources are released. On success, ownership of supplied stdin,
    /// stdout, and stderr files transfers to the operation. Reservation
    /// precedes worker-side preparation, which may truncate outputs.
    /// Rejected admission returns the handles; later preparation or
    /// execution failures use operation outcome and recovery paths and do not
    /// promise unchanged files. The scope binds the operation to this executor
    /// group and supplies its context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and
    ///   deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation
    /// fails, the estimated file bytes exceed the configured budget,
    /// executor capacity is full or closed, or manager admission
    /// is denied. Scoped variants can also fail to bind the operation group or
    /// reach the scope deadline. The failure carries the runner and
    /// command; its options retain the owned file handles on rejected
    /// admission.
    #[cfg(feature = "async")]
    pub fn submit_async_resources_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let bytes = resource_bytes(&options);
        self.submit_async_resources_in_with_bytes(scope, runner, command, options, bytes)
    }
    /// Submits resource options with a precomputed byte estimate.
    ///
    /// # Parameters
    /// * `scope`: Scope that binds the executor operation group.
    /// * `runner`: Runner configuration for the child process.
    /// * `command`: Command to submit.
    /// * `options`: Resource-backed stream options.
    /// * `bytes`: File byte estimate used for capacity admission.
    #[cfg(feature = "async")]
    pub(in crate::execution) fn submit_async_resources_in_with_bytes(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        bytes: usize,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_async_resources_manager(
            &manager,
            runner,
            command,
            options,
            bytes,
            AsyncSubmissionContext::plain(Some(scope.context())),
        )
    }
    /// Attempts to submit a command without waiting for executor capacity. Use
    /// the waiting submission methods when the caller intends to wait for
    /// local admission capacity. The returned operation has a one-shot
    /// outcome and a separate settlement phase; observing the outcome alone
    /// does not prove process and I/O resources are released. The scope
    /// binds the operation to this executor group and supplies its context
    /// and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and
    ///   deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandSubmitFailure`] when request snapshot creation fails,
    /// executor capacity is full or closed, or manager admission
    /// is denied. Scoped variants can also fail to bind the operation group
    /// or reach the scope deadline. The failure carries the runner, command,
    /// and options.
    pub fn submit_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_in_manager(&manager, runner, command, options, Some(scope.context()))
    }
    /// Attempts to submit a command without waiting for executor capacity. Use
    /// the waiting submission methods when the caller intends to wait for
    /// local admission capacity. The returned operation has a one-shot
    /// outcome and a separate settlement phase; observing the outcome alone
    /// does not prove process and I/O resources are released. On success,
    /// ownership of supplied stdin, stdout, and stderr files transfers to
    /// the operation. Reservation precedes worker-side preparation, which may
    /// truncate outputs. Rejected admission returns the handles; later
    /// preparation or execution failures use operation outcome and recovery
    /// paths and do not promise unchanged files. The scope
    /// binds the operation to this executor group and supplies its context and
    /// deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and
    ///   deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    /// Returns [`CommandResourceSubmitFailure`] when request snapshot creation
    /// fails, the estimated file bytes exceed the configured budget,
    /// executor capacity is full or closed, or manager admission
    /// is denied. Scoped variants can also fail to bind the operation group or
    /// reach the scope deadline. The failure carries the runner and
    /// command; its options retain the owned file handles on rejected
    /// admission.
    pub fn submit_resources_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let bytes = resource_bytes(&options);
        self.submit_resources_in_with_bytes(scope, runner, command, options, bytes)
    }
    /// Submits resource options with a precomputed byte estimate.
    ///
    /// # Parameters
    /// * `scope`: Scope that binds the executor operation group.
    /// * `runner`: Runner configuration for the child process.
    /// * `command`: Command to submit.
    /// * `options`: Resource-backed stream options.
    /// * `bytes`: File byte estimate used for capacity admission.
    pub(in crate::execution) fn submit_resources_in_with_bytes(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        bytes: usize,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_resources_manager(&manager, runner, command, options, bytes, Some(scope.context()))
    }
    /// Performs the submit in manager step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `manager`: Supplies the manager value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    fn submit_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
        deadline: Option<OperationContext>,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        let request = make_request(runner, command, RequestOptions::Standard(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Standard(options) = options else {
                unreachable!()
            };
            CommandSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options,
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let permit = match self.scheduler.try_reserve_until(0, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let prepared = Arc::new(AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            None,
        ));
        match manager.try_submit_external(OperationOptions::default(), Arc::clone(&prepared)) {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer());
                Ok(CommandOperation::new(handle, prepared.stop_diagnostic_slot()))
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared.restore().expect("unactivated submission retains request");
                let options = match request.options {
                    RequestOptions::Standard(o) => o,
                    _ => unreachable!(),
                };
                Err(CommandSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options,
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
    /// Performs the submit resources manager step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `manager`: Supplies the manager value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    pub(in crate::execution) fn submit_resources_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        bytes: usize,
        deadline: Option<OperationContext>,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let request = make_request(runner, command, RequestOptions::Resources(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Resources(options) = options else {
                unreachable!()
            };
            CommandResourceSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options: Box::new(options),
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let permit = match self.scheduler.try_reserve_until(bytes, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let prepared = Arc::new(AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            None,
        ));
        match manager.try_submit_external(OperationOptions::default(), Arc::clone(&prepared)) {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer());
                Ok(CommandOperation::new(handle, prepared.stop_diagnostic_slot()))
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared.restore().expect("unactivated submission retains request");
                let RequestOptions::Resources(options) = request.options else {
                    unreachable!()
                };
                Err(CommandResourceSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
    /// Performs the submit async in manager step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `manager`: Supplies the manager value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `request_options`: Supplies the request options value used by this
    ///   operation.
    /// * `context`: Supplies the context value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        request_options: RequestOptions,
        context: AsyncSubmissionContext,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        let AsyncSubmissionContext {
            bridge,
            keepalive,
            deadline,
        } = context;
        let RequestOptions::Standard(options) = request_options else {
            unreachable!()
        };
        let request = make_request(runner, command, RequestOptions::Standard(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Standard(options) = options else {
                unreachable!()
            };
            CommandSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options,
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let permit = match self.scheduler.try_reserve_until(0, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let mut prepared_state = AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            bridge,
        );
        #[cfg(feature = "async")]
        if let Some(keepalive) = keepalive {
            prepared_state = prepared_state.keep_session_alive(keepalive);
        }
        let prepared = Arc::new(prepared_state);
        match manager
            .as_async()
            .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
        {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer().as_sync());
                Ok(super::AsyncCommandOperation::new(
                    handle,
                    prepared.stop_diagnostic_slot(),
                ))
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("rejected external submission retains request");
                let RequestOptions::Standard(options) = request.options else {
                    unreachable!()
                };
                Err(CommandSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options,
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
    /// Starts a managed asynchronous command session and returns its stdin,
    /// stdout, and stderr endpoints with the operation handle. Admission is
    /// attempted immediately; use the async waiting submission methods when
    /// capacity waiting is required. Session outcome and settlement are
    /// separate observations.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] for snapshot creation, capacity, or
    /// manager admission failures.
    #[cfg(feature = "async")]
    pub(crate) fn start_session_managed(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        self.start_session_managed_in_manager(&self.manager, runner, command, options, None)
    }
    /// Starts a managed asynchronous command session and returns its stdin,
    /// stdout, and stderr endpoints with the operation handle. Admission is
    /// attempted immediately; use the async waiting submission methods when
    /// capacity waiting is required. Session outcome and settlement are
    /// separate observations. The supplied scope binds the operation to this
    /// executor group and supplies its original context and deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and
    ///   deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] for snapshot creation, capacity, group binding,
    /// deadline, or manager admission failures.
    #[cfg(feature = "async")]
    pub(crate) fn start_session_managed_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let manager = scope
            .manager()
            .with_group(&self.group)
            .map_err(|e| managed_failure(format!("group binding failed: {e}")))?;
        self.start_session_managed_in_manager(&manager, runner, command, options, Some(scope.context()))
    }
    /// Starts a managed asynchronous command session and returns its stdin,
    /// stdout, and stderr endpoints with the operation handle. Admission is
    /// attempted immediately; use the async waiting submission methods when
    /// capacity waiting is required. Session outcome and settlement are
    /// separate observations. Owned file reservation precedes worker-side
    /// preparation, which may truncate outputs. This helper maps admission
    /// rejection to [`CommandError`], so the returned error does not
    /// recover the supplied file handles; later preparation failures follow
    /// operation and recovery paths.
    ///
    /// # Parameters
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] for snapshot creation, file-byte capacity, or
    /// manager admission failures.
    #[cfg(feature = "async")]
    pub(crate) fn start_session_resources_managed(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        self.start_session_resources_managed_in_manager(&self.manager, runner, command, options, None)
    }
    /// Starts a managed asynchronous command session and returns its stdin,
    /// stdout, and stderr endpoints with the operation handle. Admission is
    /// attempted immediately; use the async waiting submission methods when
    /// capacity waiting is required. Session outcome and settlement are
    /// separate observations. Owned file reservation precedes worker-side
    /// preparation, which may truncate outputs. This helper maps admission
    /// rejection to [`CommandError`], so the returned error does not
    /// recover the supplied file handles; later preparation failures follow
    /// operation and recovery paths. The supplied scope binds the operation
    /// to this executor group and supplies its original context and
    /// deadline.
    ///
    /// # Parameters
    /// * `scope`: Identifies the operation group and supplies its context and
    ///   deadline.
    /// * `runner`: Configures how the child process is launched and managed.
    /// * `command`: Describes the program, arguments, and standard stream
    ///   configuration.
    /// * `options`: Selects standard streams and command execution behavior.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] for snapshot creation, file-byte capacity,
    /// group binding, deadline, or manager admission failures.
    #[cfg(feature = "async")]
    pub(crate) fn start_session_resources_managed_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let manager = scope
            .manager()
            .with_group(&self.group)
            .map_err(|e| managed_failure(format!("group binding failed: {e}")))?;
        self.start_session_resources_managed_in_manager(&manager, runner, command, options, Some(scope.context()))
    }
    /// Performs the start session managed in manager step in the managed
    /// command execution lifecycle.
    ///
    /// # Parameters
    /// * `manager`: Supplies the manager value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    #[cfg(feature = "async")]
    fn start_session_managed_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
        deadline: Option<OperationContext>,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let channels = session_channels(&runner);
        let crate::session::channel::SessionChannels {
            bridge,
            stdin,
            stdout,
            stderr,
            stdin_reader,
            stdout_writer,
            stderr_writer,
        } = channels;
        let operation = self
            .submit_async_in_manager(
                manager,
                runner,
                command,
                RequestOptions::Standard(options),
                AsyncSubmissionContext {
                    bridge: Some(bridge),
                    keepalive: Some(super::adapter::SessionOwners {
                        _stdin: stdin_reader,
                        _stdout: stdout_writer,
                        _stderr: stderr_writer,
                    }),
                    deadline,
                },
            )
            .map_err(|e| managed_failure(format!("managed session admission failed: {:?}", e.reason)))?;
        Ok(crate::session::CommandSession {
            operation: Some(operation.into_sync()),
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
        })
    }
    /// Performs the start session resources managed in manager step in the
    /// managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `manager`: Supplies the manager value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    /// * `deadline`: Supplies the deadline value used by this operation.
    #[cfg(feature = "async")]
    fn start_session_resources_managed_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        deadline: Option<OperationContext>,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let channels = session_channels(&runner);
        let crate::session::channel::SessionChannels {
            bridge,
            stdin,
            stdout,
            stderr,
            stdin_reader,
            stdout_writer,
            stderr_writer,
        } = channels;
        let bytes = resource_bytes(&options);
        let operation = self
            .submit_async_resources_manager(
                manager,
                runner,
                command,
                options,
                bytes,
                AsyncSubmissionContext {
                    bridge: Some(bridge),
                    keepalive: Some(super::adapter::SessionOwners {
                        _stdin: stdin_reader,
                        _stdout: stdout_writer,
                        _stderr: stderr_writer,
                    }),
                    deadline,
                },
            )
            .map_err(|e| managed_failure(format!("managed resource session admission failed: {:?}", e.reason)))?;
        Ok(crate::session::CommandSession {
            operation: Some(operation.into_sync()),
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
        })
    }
    /// Performs the submit async resources manager step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `manager`: Supplies the manager value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    /// * `context`: Supplies the context value used by this operation.
    #[cfg(feature = "async")]
    pub(in crate::execution) fn submit_async_resources_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        bytes: usize,
        context: AsyncSubmissionContext,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let AsyncSubmissionContext {
            bridge,
            keepalive,
            deadline,
        } = context;
        let request = make_request(runner, command, RequestOptions::Resources(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Resources(options) = options else {
                unreachable!()
            };
            CommandResourceSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options: Box::new(options),
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let permit = match self.scheduler.try_reserve_until(bytes, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let mut prepared_state = AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            bridge,
        );
        if let Some(keepalive) = keepalive {
            prepared_state = prepared_state.keep_session_alive(keepalive)
        }
        let prepared = Arc::new(prepared_state);
        match manager
            .as_async()
            .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
        {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer().as_sync());
                Ok(super::AsyncCommandOperation::new(
                    handle,
                    prepared.stop_diagnostic_slot(),
                ))
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("rejected external submission retains request");
                let RequestOptions::Resources(options) = request.options else {
                    unreachable!()
                };
                Err(CommandResourceSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
}
/// Estimates existing input and output file lengths for resource admission.
///
/// # Parameters
/// * `options`: Supplies resource file handles whose current metadata is read.
///
/// Metadata lookup failures contribute zero bytes. Summation saturates at
/// `usize::MAX` so a large combined estimate cannot wrap below the limit.
pub(in crate::execution) fn resource_bytes(options: &CommandResourceRunOptions) -> usize {
    [
        options
            .configured_stdin()
            .and_then(|f| f.metadata().ok())
            .map(|m| m.len()),
        options
            .configured_stdout()
            .and_then(|(f, _)| f.metadata().ok())
            .map(|m| m.len()),
        options
            .configured_stderr()
            .and_then(|(f, _)| f.metadata().ok())
            .map(|m| m.len()),
    ]
    .into_iter()
    .flatten()
    .fold(0usize, |sum, n| {
        sum.saturating_add(usize::try_from(n).unwrap_or(usize::MAX))
    })
}

#[cfg(test)]
mod resource_byte_tests {
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;

    use super::resource_bytes;
    use crate::Command;
    use crate::CommandExecutionConfig;
    use crate::CommandExecutor;
    use crate::CommandResourceRunOptions;
    use crate::CommandRunner;
    use crate::execution::CapacityFailure;
    use crate::execution::CommandSubmitFailureReason;
    use crate::resource_run_options::FileSinkPolicy;

    #[test]
    fn test_resource_byte_estimates_sum_all_present_files() {
        let mut stdin = tempfile::tempfile().unwrap();
        stdin.write_all(b"input").unwrap();
        let mut stdout = tempfile::tempfile().unwrap();
        stdout.write_all(b"out").unwrap();
        let mut stderr = tempfile::tempfile().unwrap();
        stderr.write_all(b"error!").unwrap();
        let options = CommandResourceRunOptions::new()
            .stdin_file(stdin)
            .stdout_sink(stdout, FileSinkPolicy::Preserve)
            .stderr_sink(stderr, FileSinkPolicy::Truncate);

        assert_eq!(resource_bytes(&options), 14);
    }

    #[test]
    fn test_resource_capacity_rejection_returns_the_original_input_file() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = CommandExecutor::new(
            manager,
            CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                max_waiters: 1,
                max_queued_input_bytes: 2,
                max_diagnostics: 1,
            },
        )
        .unwrap();
        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"larger than budget").unwrap();
        let failure = match executor.submit_resources(
            CommandRunner::without_timeout(),
            Command::new("must-not-run-after-input-limit-rejection"),
            CommandResourceRunOptions::new().stdin_file(input),
        ) {
            Ok(_) => panic!("resource input over the byte budget must be rejected"),
            Err(failure) => failure,
        };

        assert_eq!(
            failure.reason,
            CommandSubmitFailureReason::Capacity(CapacityFailure::InputBytesExceeded)
        );
        let mut returned_input = failure
            .options
            .configured_stdin()
            .expect("the rejected input owner should be returned")
            .try_clone()
            .unwrap();
        returned_input.seek(SeekFrom::Start(0)).unwrap();
        let mut contents = Vec::new();
        returned_input.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, b"larger than budget");
        let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Drain);
    }

    #[test]
    fn test_closed_executor_rejects_standard_and_resource_submissions() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
        let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);

        let standard = match executor.submit(
            CommandRunner::without_timeout(),
            Command::new("must-not-run-after-shutdown"),
            Default::default(),
        ) {
            Ok(_) => panic!("shutdown closes admission before a command is accepted"),
            Err(failure) => failure,
        };
        assert_eq!(
            standard.reason,
            CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
        );
        assert_eq!(standard.command.program(), "must-not-run-after-shutdown");

        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"preserved after shutdown").unwrap();
        let resource = match executor.submit_resources(
            CommandRunner::without_timeout(),
            Command::new("must-not-run-with-resource-after-shutdown"),
            CommandResourceRunOptions::new().stdin_file(input),
        ) {
            Ok(_) => panic!("shutdown must return resources for rejected submissions"),
            Err(failure) => failure,
        };
        assert_eq!(
            resource.reason,
            CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
        );
        let mut returned_input = resource
            .options
            .configured_stdin()
            .expect("rejected submission retains its original input file")
            .try_clone()
            .unwrap();
        returned_input.seek(SeekFrom::Start(0)).unwrap();
        let mut contents = Vec::new();
        returned_input.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, b"preserved after shutdown");
    }

    #[test]
    fn test_manager_admission_rejection_restores_standard_and_resource_requests() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).unwrap();
        let _shutdown = executor.group.request_shutdown(qubit_operation::ShutdownMode::Cancel);

        let standard = match executor.submit(
            CommandRunner::without_timeout(),
            Command::new("preserved-after-manager-rejection"),
            Default::default(),
        ) {
            Ok(_) => panic!("manager shutdown should reject external operation admission"),
            Err(failure) => failure,
        };
        assert!(matches!(standard.reason, CommandSubmitFailureReason::Admission(_)));
        assert_eq!(standard.command.program(), "preserved-after-manager-rejection");

        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"restored after manager rejection").unwrap();
        let resource = match executor.submit_resources(
            CommandRunner::without_timeout(),
            Command::new("preserved-resource-after-manager-rejection"),
            CommandResourceRunOptions::new().stdin_file(input),
        ) {
            Ok(_) => panic!("manager shutdown should reject resource operation admission"),
            Err(failure) => failure,
        };
        assert!(matches!(resource.reason, CommandSubmitFailureReason::Admission(_)));
        assert_eq!(resource.command.program(), "preserved-resource-after-manager-rejection");

        let mut returned_input = resource
            .options
            .configured_stdin()
            .expect("rejected resource submission should restore its input owner")
            .try_clone()
            .unwrap();
        returned_input.seek(SeekFrom::Start(0)).unwrap();
        let mut contents = Vec::new();
        returned_input.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, b"restored after manager rejection");
    }

    #[cfg(feature = "async")]
    #[test]
    fn test_async_manager_admission_rejection_restores_standard_and_resource_requests() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
        let _shutdown = executor.group.request_shutdown(qubit_operation::ShutdownMode::Cancel);

        let standard = match executor.submit_async(
            CommandRunner::without_timeout(),
            Command::new("preserved-async-after-manager-rejection"),
            Default::default(),
        ) {
            Ok(_) => panic!("closed group should reject async external operation admission"),
            Err(failure) => failure,
        };
        assert!(matches!(standard.reason, CommandSubmitFailureReason::Admission(_)));
        assert_eq!(standard.command.program(), "preserved-async-after-manager-rejection");

        let mut input = tempfile::tempfile().unwrap();
        input
            .write_all(b"restored async resource after manager rejection")
            .unwrap();
        let resource = match executor.submit_async_resources(
            CommandRunner::without_timeout(),
            Command::new("preserved-async-resource-after-manager-rejection"),
            CommandResourceRunOptions::new().stdin_file(input),
        ) {
            Ok(_) => panic!("closed group should reject async resource admission"),
            Err(failure) => failure,
        };
        assert!(matches!(resource.reason, CommandSubmitFailureReason::Admission(_)));
        assert_eq!(
            resource.command.program(),
            "preserved-async-resource-after-manager-rejection"
        );

        let mut returned_input = resource
            .options
            .configured_stdin()
            .expect("rejected async resource submission should restore its input owner")
            .try_clone()
            .unwrap();
        returned_input.seek(SeekFrom::Start(0)).unwrap();
        let mut contents = Vec::new();
        returned_input.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, b"restored async resource after manager rejection");
    }
}

/// Waits asynchronously until capacity changes, scope stop, or deadline.
///
/// # Parameters
/// * `scheduler`: Supplies the scheduler being observed.
/// * `bytes`: Supplies the request's estimated input size.
/// * `context`: Supplies an optional scope context.
///
/// # Errors
/// Returns a waiter quota failure when no scheduler waiter slot is available.
#[cfg(feature = "async")]
async fn wait_capacity_async(
    scheduler: BoundedScheduler,
    bytes: usize,
    context: Option<OperationContext>,
) -> Result<bool, super::CommandSubmitWaitFailure> {
    let mut capacity = Box::pin(CapacityWait {
        scheduler,
        bytes,
        waiter: None,
    });
    if let Some(context) = context {
        let mut stop = Box::pin(context.stop_requested());
        let mut deadline = context.deadline_signal().map(Box::pin);
        std::future::poll_fn(|cx| {
            if stop.as_mut().poll(cx).is_ready() {
                return std::task::Poll::Ready(Ok(false));
            }
            if deadline
                .as_mut()
                .is_some_and(|signal| signal.as_mut().poll(cx).is_ready())
            {
                return std::task::Poll::Ready(Ok(false));
            }
            capacity.as_mut().poll(cx)
        })
        .await
    } else {
        capacity.await
    }
}

/// Waits for a manager Busy retry tick while holding a waiter registration.
///
/// # Parameters
/// * `scheduler`: Supplies the scheduler whose quota and close state are
///   observed.
/// * `context`: Supplies an optional scope context.
///
/// # Errors
/// Returns a waiter quota failure when no scheduler waiter slot is available.
#[cfg(feature = "async")]
async fn wait_busy_async(
    scheduler: BoundedScheduler,
    context: Option<OperationContext>,
) -> Result<bool, super::CommandSubmitWaitFailure> {
    let mut waiter: Option<SchedulerWaiterGuard> = None;
    let mut timer = Box::pin(wait_retry_tick(context.clone()));
    std::future::poll_fn(|cx| {
        if context
            .as_ref()
            .is_some_and(|context| context.is_expired() || context.stop_reason().is_some())
        {
            return std::task::Poll::Ready(Ok(false));
        }
        if scheduler.is_closed() {
            return std::task::Poll::Ready(Ok(false));
        }
        if let Some(waiter) = &waiter {
            scheduler.update_waiter(waiter.id, cx.waker());
        } else {
            match scheduler.register_waiter(Some(cx.waker())) {
                Ok(id) => {
                    waiter = Some(SchedulerWaiterGuard {
                        scheduler: scheduler.clone(),
                        id,
                    })
                }
                Err(CapacityFailure::QueueFull) => {
                    return std::task::Poll::Ready(Err(super::CommandSubmitWaitFailure::WaiterLimit));
                }
                Err(CapacityFailure::Closed) => return std::task::Poll::Ready(Ok(false)),
                Err(_) => return std::task::Poll::Ready(Ok(false)),
            }
        }
        timer
            .as_mut()
            .poll(cx)
            .map(|result| result.map_err(super::CommandSubmitWaitFailure::from))
    })
    .await
}

/// Performs the wait retryable async step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `scheduler`: Supplies the scheduler whose waiter quota is used.
/// * `bytes`: Supplies the request's estimated input size.
/// * `reason`: Supplies the rejection reason that selects the wait policy.
/// * `context`: Supplies an optional scope context.
///
/// # Errors
/// Returns a waiter quota failure when no scheduler waiter slot is available.
#[cfg(feature = "async")]
pub(in crate::execution) async fn wait_retryable_async(
    scheduler: BoundedScheduler,
    bytes: usize,
    reason: &CommandSubmitFailureReason,
    context: Option<OperationContext>,
) -> Result<bool, super::CommandSubmitWaitFailure> {
    if matches!(
        reason,
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull)
    ) {
        wait_capacity_async(scheduler, bytes, context).await
    } else if matches!(
        reason,
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy)
    ) {
        wait_busy_async(scheduler, context).await
    } else {
        Ok(false)
    }
}

/// Stores the wait retry tick value used by this execution component.
///
/// # Parameters
/// * `context`: Supplies the context value used by this operation.
#[cfg(feature = "async")]
pub(in crate::execution) async fn wait_retry_tick(
    context: Option<OperationContext>,
) -> Result<bool, RetryTimerFailure> {
    let mut delay = Box::pin(RetryDelay::new());
    if let Some(context) = context {
        let mut stop = Box::pin(context.stop_requested());
        let mut deadline = context.deadline_signal().map(Box::pin);
        std::future::poll_fn(|cx| {
            if stop.as_mut().poll(cx).is_ready() {
                return std::task::Poll::Ready(Ok(false));
            }
            if deadline
                .as_mut()
                .is_some_and(|signal| signal.as_mut().poll(cx).is_ready())
            {
                return std::task::Poll::Ready(Ok(false));
            }
            let delay_result: std::task::Poll<Result<(), RetryTimerFailure>> = delay.as_mut().poll(cx);
            if let std::task::Poll::Ready(value) = delay_result {
                return std::task::Poll::Ready(value.map(|()| true));
            }
            std::task::Poll::Pending
        })
        .await
    } else {
        let result: Result<(), RetryTimerFailure> = delay.await;
        result.map(|()| true)
    }
}
/// Stores the managed failure value used by this execution component.
///
/// # Parameters
/// * `message`: Supplies the message value used by this operation.
#[cfg(feature = "async")]
fn managed_failure(message: String) -> CommandError {
    CommandError::from_reason(
        "<managed command>",
        crate::CommandErrorReason::ManagedOperationFailed { diagnostic: message },
        None,
    )
}
/// Performs the make request step in the managed command execution lifecycle.
///
/// # Parameters
/// * `runner`: Supplies the runner value used by this operation.
/// * `command`: Supplies the command value used by this operation.
/// * `options`: Supplies the options value used by this operation.
fn make_request(
    runner: CommandRunner,
    command: Command,
    options: RequestOptions,
) -> Result<Request, Box<(CommandRunner, Command, RequestOptions, String)>> {
    let stdin = command.stdin_file_path().map(ToOwned::to_owned);
    let (stdout, stderr) = match &options {
        RequestOptions::Standard(o) => (
            o.configured_stdout_file().map(ToOwned::to_owned),
            o.configured_stderr_file().map(ToOwned::to_owned),
        ),
        RequestOptions::Resources(o) => (
            o.ordinary_options().configured_stdout_file().map(ToOwned::to_owned),
            o.ordinary_options().configured_stderr_file().map(ToOwned::to_owned),
        ),
    };
    let needs_base = [stdin.as_deref(), stdout.as_deref(), stderr.as_deref()]
        .into_iter()
        .flatten()
        .any(needs_parent_directory);
    let wd = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) if needs_base => {
            return Err(Box::new((
                runner,
                command,
                options,
                format!("parent directory snapshot failed: {error}"),
            )));
        }
        Err(_) => std::path::PathBuf::new(),
    };
    let resolve =
        |path: Option<std::path::PathBuf>| path.map(|path| crate::execution::files::resolve_io_path(&wd, &path));
    let paths = IoPathSnapshot {
        stdin: resolve(stdin),
        stdout: resolve(stdout),
        stderr: resolve(stderr),
    };
    Ok(Request {
        operation_id: None,
        runner,
        command,
        options,
        snapshot: ExecutionSnapshot {
            working_directory: wd,
            environment: Vec::new(),
        },
        io_paths: paths,
        deadline: None,
    })
}

/// Reports whether a path needs the captured parent directory as its base.
///
/// # Parameters
/// * `path`: Path tested for relative path semantics.
fn needs_parent_directory(path: &std::path::Path) -> bool {
    !path.is_absolute() && !has_platform_prefix(path)
}

/// Returns whether a Windows path has a drive or namespace prefix.
///
/// # Parameters
/// * `path`: Path whose first component is inspected.
#[cfg(windows)]
fn has_platform_prefix(path: &std::path::Path) -> bool {
    use std::path::Component;
    matches!(path.components().next(), Some(Component::Prefix(_)))
}

/// Returns false because non-Windows paths have no Windows platform prefix.
///
/// # Parameters
/// * `_`: Unused path on non-Windows targets.
#[cfg(not(windows))]
fn has_platform_prefix(_: &std::path::Path) -> bool {
    false
}

#[cfg(test)]
mod executor_decision_tests {
    use std::path::Path;
    use std::task::Waker;

    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::TimerRegistration;

    use super::deadline_failure;
    #[cfg(feature = "async")]
    use super::futures_lite_block_on;
    use super::make_request;
    use super::retry_wait;
    use crate::Command;
    use crate::CommandRunOptions;
    use crate::CommandRunner;
    use crate::execution::CommandExecutionConfig;
    use crate::execution::CommandSubmitFailureReason;
    use crate::execution::SnapshotFailure;
    use crate::execution::contracts::CapacityFailure;
    use crate::execution::scheduler::BoundedScheduler;

    #[test]
    fn test_two_executors_keep_waiter_quotas_local_during_concurrent_submissions() {
        use std::time::Instant;

        use qubit_operation::OperationManager;

        use crate::CommandExecutor;
        use crate::execution::CommandExecutorApi;
        use crate::execution::contracts::Scheduler;

        let config = CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        };
        let first = CommandExecutor::new(OperationManager::new(Default::default()).unwrap(), config.clone()).unwrap();
        let second = CommandExecutor::new(OperationManager::new(Default::default()).unwrap(), config).unwrap();
        let first_reservation = first.scheduler.try_reserve(0).unwrap();
        let second_reservation = second.scheduler.try_reserve(0).unwrap();
        let first_executor = first.clone();
        let second_executor = second.clone();
        let (first_tx, first_rx) = std::sync::mpsc::channel();
        let (second_tx, second_rx) = std::sync::mpsc::channel();
        let first_waiter = std::thread::spawn(move || {
            let failure = match first_executor.submit_wait_blocking(
                CommandRunner::without_timeout(),
                Command::new("first-executor-request"),
                CommandRunOptions::new(),
            ) {
                Err(failure) => failure,
                Ok(_) => panic!("full scheduler should keep the first request waiting"),
            };
            first_tx.send(failure).unwrap();
        });
        let second_waiter = std::thread::spawn(move || {
            let failure = match second_executor.submit_wait_blocking(
                CommandRunner::without_timeout(),
                Command::new("second-executor-request"),
                CommandRunOptions::new(),
            ) {
                Err(failure) => failure,
                Ok(_) => panic!("full scheduler should keep the second request waiting"),
            };
            second_tx.send(failure).unwrap();
        });
        let setup_deadline = Instant::now() + std::time::Duration::from_secs(2);
        while (first.scheduler.snapshot().waiters == 0 || second.scheduler.snapshot().waiters == 0)
            && Instant::now() < setup_deadline
        {
            std::thread::yield_now();
        }
        assert_eq!(first.scheduler.snapshot().waiters, 1);
        assert_eq!(second.scheduler.snapshot().waiters, 1);
        first.scheduler.close_admission();
        second.scheduler.close_admission();

        let first_failure = first_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        let second_failure = second_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        first_waiter.join().unwrap();
        second_waiter.join().unwrap();
        assert_eq!(
            first_failure.command.program(),
            std::ffi::OsStr::new("first-executor-request")
        );
        assert_eq!(
            second_failure.command.program(),
            std::ffi::OsStr::new("second-executor-request")
        );
        assert_eq!(first.scheduler.snapshot().waiters, 0);
        assert_eq!(second.scheduler.snapshot().waiters, 0);
        drop(first_reservation);
        drop(second_reservation);
    }

    #[test]
    fn test_waiter_limit_is_returned_by_public_blocking_wait_entry() {
        use qubit_operation::OperationManager;

        use crate::CommandExecutor;
        use crate::execution::CommandExecutorApi;
        use crate::execution::contracts::Scheduler;

        let manager = OperationManager::new(Default::default()).unwrap();
        let executor = CommandExecutor::new(
            manager,
            CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                max_waiters: 0,
                ..Default::default()
            },
        )
        .unwrap();
        let reservation = executor.scheduler.try_reserve(0).unwrap();
        let failure = match executor.submit_wait_blocking(
            CommandRunner::without_timeout(),
            Command::new("preserved-command").arg("preserved-arg"),
            CommandRunOptions::new().tee_stdout_to_file("preserved-output.log"),
        ) {
            Err(failure) => failure,
            Ok(_) => panic!("zero waiter capacity should reject a retry wait"),
        };
        assert_eq!(
            failure.reason,
            CommandSubmitFailureReason::Wait(crate::execution::CommandSubmitWaitFailure::WaiterLimit)
        );
        assert_eq!(failure.command.program(), std::ffi::OsStr::new("preserved-command"));
        assert_eq!(
            failure.command.arguments().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("preserved-arg")]
        );
        assert_eq!(
            failure.options.configured_stdout_file(),
            Some(Path::new("preserved-output.log"))
        );
        drop(reservation);
    }

    #[test]
    fn test_scoped_blocking_wait_returns_stopped_with_original_request_while_full() {
        use std::time::Duration;
        use std::time::Instant;

        use qubit_operation::OperationManager;
        use qubit_operation::StopReason;

        use crate::CommandExecutor;
        use crate::execution::CommandExecutorApi;
        use crate::execution::contracts::Scheduler;

        let manager = OperationManager::new(Default::default()).unwrap();
        let executor = CommandExecutor::new(
            manager.clone(),
            CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                max_waiters: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let reservation = executor.scheduler.try_reserve(0).unwrap();
        let (context_tx, context_rx) = std::sync::mpsc::channel();
        let (failure_tx, failure_rx) = std::sync::mpsc::channel();
        let waiting_executor = executor.clone();
        let submitter = std::thread::spawn(move || {
            let outcome = manager
                .run(Default::default(), move |scope| {
                    context_tx.send(scope.context()).unwrap();
                    let failure = match waiting_executor.submit_wait_blocking_in(
                        &scope,
                        CommandRunner::without_timeout().working_directory("retained-directory"),
                        Command::new("must-not-start").arg("original-arg"),
                        CommandRunOptions::new().tee_stdout_to_file("original-output.log"),
                    ) {
                        Err(failure) => failure,
                        Ok(_) => panic!("scope stop should reject the waiting submission"),
                    };
                    failure_tx.send(failure).unwrap();
                    Ok::<_, ()>(())
                })
                .unwrap();
            drop(outcome);
        });
        let context = context_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let setup_deadline = Instant::now() + Duration::from_secs(2);
        while executor.scheduler.snapshot().waiters == 0 && Instant::now() < setup_deadline {
            std::thread::yield_now();
        }
        assert_eq!(executor.scheduler.snapshot().waiters, 1);
        assert!(context.request_stop(StopReason::UserCancelled));

        let failure = failure_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("scoped stop should return the original waiting request promptly");
        assert_eq!(
            failure.reason,
            CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Stopped)
        );
        assert_eq!(failure.command.program(), std::ffi::OsStr::new("must-not-start"));
        assert_eq!(
            failure.command.arguments().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("original-arg")]
        );
        assert_eq!(
            failure.runner.configured_working_directory(),
            Some(Path::new("retained-directory"))
        );
        assert_eq!(
            failure.options.configured_stdout_file(),
            Some(Path::new("original-output.log"))
        );
        assert_eq!(executor.scheduler.snapshot().waiters, 0);
        assert_eq!(executor.scheduler.snapshot().active, 1);
        drop(reservation);
        submitter.join().unwrap();
    }

    #[test]
    fn test_make_request_captures_parent_io_base_without_copying_environment() {
        let parent = std::env::current_dir().expect("parent directory should be available");
        let runner_directory = Path::new("runner-default");
        let command_directory = Path::new("command-override");
        let command = Command::new("worker")
            .stdin_file("input.txt")
            .working_directory(command_directory);
        let request = make_request(
            CommandRunner::without_timeout().working_directory(runner_directory),
            command,
            super::RequestOptions::Standard(CommandRunOptions::new().tee_stdout_to_file("output.log")),
        )
        .expect("request snapshot should succeed");

        assert_eq!(request.snapshot.working_directory, parent);
        assert!(request.snapshot.environment.is_empty());
        assert_eq!(
            request.io_paths.stdin.as_deref(),
            Some(parent.join("input.txt").as_path())
        );
        assert_eq!(
            request.io_paths.stdout.as_deref(),
            Some(parent.join("output.log").as_path())
        );
        assert_eq!(request.command.stdin_file_path(), Some(Path::new("input.txt")));
        assert_eq!(request.command.working_directory_override(), Some(command_directory));
        assert_eq!(request.runner.configured_working_directory(), Some(runner_directory));
    }

    struct FixedClock(ClockDomainId);

    impl ClockSource for FixedClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.0, 0)
        }

        fn register_deadline(&self, _: MonotonicTime, _: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    struct NoopRegistration;

    impl TimerRegistration for NoopRegistration {
        fn cancel(self: Box<Self>) {}
    }

    #[test]
    fn test_retry_decision_preserves_other_failures_and_retries_manager_busy() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig::default()).unwrap();
        let context = OperationContext::new(ContextOptions::default()).unwrap();
        let snapshot = CommandSubmitFailureReason::Snapshot(SnapshotFailure {
            diagnostic: "invalid snapshot".to_owned(),
        });

        assert!(!retry_wait(&snapshot, &scheduler, 0, None).unwrap());
        assert_eq!(deadline_failure(snapshot.clone(), &context), snapshot);
        assert!(
            retry_wait(
                &CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy),
                &scheduler,
                0,
                None,
            )
            .unwrap()
        );
    }

    #[test]
    fn test_busy_wait_shares_waiter_quota_with_capacity_wait() {
        use std::time::Instant;

        use crate::execution::contracts::Scheduler;

        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let waiting_scheduler = scheduler.clone();
        let capacity_waiter = std::thread::spawn(move || {
            retry_wait(
                &CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull),
                &waiting_scheduler,
                0,
                None,
            )
        });
        let setup_deadline = Instant::now() + std::time::Duration::from_secs(2);
        while scheduler.snapshot().waiters == 0 && Instant::now() < setup_deadline {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.snapshot().waiters, 1);

        let busy = CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy);
        assert_eq!(
            retry_wait(&busy, &scheduler, 0, None),
            Err(crate::execution::CommandSubmitWaitFailure::WaiterLimit)
        );
        drop(reservation);
        assert!(capacity_waiter.join().unwrap().unwrap());
        assert_eq!(scheduler.snapshot().waiters, 0);
    }

    #[test]
    fn test_busy_wait_observes_scope_stop() {
        use std::time::Instant;

        use qubit_operation::StopReason;

        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let context = OperationContext::new(ContextOptions::default()).unwrap();
        let waiting_scheduler = scheduler.clone();
        let waiting_context = context.clone();
        let waiter = std::thread::spawn(move || {
            retry_wait(
                &CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy),
                &waiting_scheduler,
                0,
                Some(waiting_context),
            )
        });
        let setup_deadline = Instant::now() + std::time::Duration::from_secs(2);
        while scheduler.snapshot().waiters == 0 && Instant::now() < setup_deadline {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.snapshot().waiters, 1);
        assert!(context.request_stop(StopReason::UserCancelled));

        assert!(!waiter.join().unwrap().unwrap());
        assert_eq!(scheduler.snapshot().waiters, 0);
    }

    #[test]
    fn test_busy_wait_returns_when_scheduler_closes() {
        use std::time::Instant;

        use crate::execution::contracts::Scheduler;

        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let busy = CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy);
        let waiting_scheduler = scheduler.clone();
        let waiter = std::thread::spawn(move || retry_wait(&busy, &waiting_scheduler, 0, None));
        let setup_deadline = Instant::now() + std::time::Duration::from_secs(2);
        while scheduler.snapshot().waiters == 0 && Instant::now() < setup_deadline {
            std::thread::yield_now();
        }
        assert_eq!(scheduler.snapshot().waiters, 1);
        scheduler.close_admission();

        assert!(!waiter.join().unwrap().unwrap());
        assert_eq!(scheduler.snapshot().waiters, 0);
    }

    #[test]
    fn test_manager_busy_wait_respects_scheduler_waiter_limit() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_waiters: 0,
            ..Default::default()
        })
        .unwrap();
        let busy = CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy);

        assert_eq!(
            retry_wait(&busy, &scheduler, 0, None),
            Err(crate::execution::CommandSubmitWaitFailure::WaiterLimit),
            "a Busy retry must not proceed without a scheduler waiter slot"
        );
        assert_eq!(scheduler.snapshot().waiters, 0);
    }

    #[test]
    fn test_expired_deadline_replaces_retry_failure_with_deadline_failure() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig::default()).unwrap();
        let domain = ClockDomainId::new_unique();
        let context = OperationContext::new(ContextOptions {
            clock: Some(std::sync::Arc::new(FixedClock(domain))),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 0))),
            budget: None,
        })
        .unwrap();
        let busy = CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy);

        assert!(!retry_wait(&busy, &scheduler, 0, Some(context.clone())).unwrap());
        assert_eq!(
            deadline_failure(busy, &context),
            CommandSubmitFailureReason::Capacity(crate::execution::CapacityFailure::DeadlineExpired)
        );
    }

    #[test]
    fn test_wait_failure_preserves_each_retry_timer_failure() {
        use crate::execution::CommandSubmitWaitFailure;

        for timer_failure in [
            crate::execution::RetryTimerFailure::Startup,
            crate::execution::RetryTimerFailure::Full,
            crate::execution::RetryTimerFailure::Disconnected,
        ] {
            assert_eq!(
                CommandSubmitWaitFailure::from(timer_failure),
                CommandSubmitWaitFailure::Timer(timer_failure)
            );
        }
    }

    #[test]
    fn test_scoped_resource_wait_stop_preserves_request_while_capacity_is_full() {
        use std::io::Read;
        use std::io::Seek;
        use std::io::Write;
        use std::time::Duration;
        use std::time::Instant;

        use qubit_operation::OperationManager;
        use qubit_operation::StopReason;

        use crate::CommandExecutor;
        use crate::CommandExecutorApi;
        use crate::CommandResourceRunOptions;
        use crate::execution::contracts::Scheduler;

        let manager = OperationManager::new(Default::default()).unwrap();
        let executor = CommandExecutor::new(
            manager.clone(),
            CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                max_waiters: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let reservation = executor.scheduler.try_reserve(0).unwrap();
        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"retained resource").unwrap();
        input.rewind().unwrap();
        let (context_tx, context_rx) = std::sync::mpsc::channel();
        let (failure_tx, failure_rx) = std::sync::mpsc::channel();
        let waiting_executor = executor.clone();
        let submitter = std::thread::spawn(move || {
            let _outcome = manager
                .run(Default::default(), move |scope| {
                    context_tx.send(scope.context()).unwrap();
                    let failure = match waiting_executor.submit_wait_resources_blocking_in(
                        &scope,
                        CommandRunner::without_timeout(),
                        Command::new("must-not-start"),
                        CommandResourceRunOptions::new().stdin_file(input),
                    ) {
                        Err(failure) => failure,
                        Ok(_) => panic!("stopped scope must return the original request"),
                    };
                    failure_tx.send(failure).unwrap();
                    Ok::<_, ()>(())
                })
                .unwrap();
        });
        let context = context_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let timeout = Instant::now() + Duration::from_secs(2);
        while executor.scheduler.snapshot().waiters == 0 && Instant::now() < timeout {
            std::thread::yield_now();
        }
        assert_eq!(executor.scheduler.snapshot().waiters, 1);
        assert!(context.request_stop(StopReason::UserCancelled));
        let failure = failure_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        assert_eq!(
            failure.reason,
            CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Stopped)
        );
        let mut returned = failure.options.configured_stdin().unwrap().try_clone().unwrap();
        returned.rewind().unwrap();
        let mut contents = Vec::new();
        returned.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, b"retained resource");
        assert_eq!(executor.scheduler.snapshot().waiters, 0);
        assert_eq!(executor.scheduler.snapshot().active, 1);
        drop(reservation);
        submitter.join().unwrap();
    }

    #[cfg(feature = "async")]
    #[test]
    fn test_async_retry_classification_accepts_only_capacity_and_busy() {
        use super::retryable_async;
        use crate::execution::CapacityFailure;
        use crate::execution::command_executor::wait::async_capacity_retry;

        let active_full = CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull);
        let queue_full = CommandSubmitFailureReason::Capacity(CapacityFailure::QueueFull);
        let input_limit = CommandSubmitFailureReason::Capacity(CapacityFailure::InputBytesExceeded);
        let busy = CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy);
        let snapshot = CommandSubmitFailureReason::Snapshot(SnapshotFailure {
            diagnostic: "invalid snapshot".to_owned(),
        });

        assert!(async_capacity_retry(&active_full));
        assert!(async_capacity_retry(&queue_full));
        assert!(!async_capacity_retry(&busy));
        assert!(!async_capacity_retry(&input_limit));
        assert!(retryable_async(&busy));
        assert!(!retryable_async(&snapshot));
    }

    #[cfg(feature = "async")]
    #[test]
    fn test_managed_failure_preserves_diagnostic_as_structured_reason() {
        let error = super::managed_failure("executor worker unavailable".to_owned());

        assert!(matches!(
            error.reason(),
            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                if diagnostic == "executor worker unavailable"
        ));
    }

    #[cfg(feature = "async")]
    #[test]
    fn test_async_capacity_wait_returns_when_scheduler_has_room() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig::default()).unwrap();
        assert!(futures_lite_block_on(super::wait_capacity_async(scheduler, 0, None)).unwrap());
    }
}

#[cfg(all(test, unix))]
mod snapshot_failure_tests {
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;
    use std::process::Command as ProcessCommand;

    use qubit_operation::OperationManager;

    use crate::Command;
    use crate::CommandExecutionConfig;
    use crate::CommandExecutor;
    use crate::CommandResourceRunOptions;
    use crate::CommandRunOptions;
    use crate::CommandRunner;
    use crate::execution::CommandSubmitFailureReason;

    const CHILD_MARKER: &str = "QUBIT_COMMAND_SNAPSHOT_FAILURE_CHILD";

    #[test]
    fn test_resource_submission_returns_input_when_working_directory_snapshot_fails() {
        if std::env::var_os(CHILD_MARKER).is_some() {
            assert_snapshot_failure_returns_input();
            return;
        }

        let output = ProcessCommand::new(std::env::current_exe().expect("test executable should be available"))
            .arg("--exact")
            .arg("execution::executor::snapshot_failure_tests::test_resource_submission_returns_input_when_working_directory_snapshot_fails")
            .arg("--nocapture")
            .env(CHILD_MARKER, "1")
            .output()
            .expect("isolated snapshot failure test should start");
        assert!(
            output.status.success(),
            "child test failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn assert_snapshot_failure_returns_input() {
        let mut input = tempfile::tempfile().expect("stdin file should be created");
        input
            .write_all(b"recoverable input bytes")
            .expect("stdin fixture should be written");
        input.seek(SeekFrom::Start(0)).expect("stdin cursor should be reset");

        let manager = OperationManager::new(Default::default()).expect("manager should start");
        let executor =
            CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).expect("executor should start");
        let removed_directory = tempfile::tempdir().expect("temporary directory should be created");
        std::env::set_current_dir(removed_directory.path()).expect("current directory should change");
        std::fs::remove_dir(removed_directory.path()).expect("current directory should be unlinked");

        let failure = match executor.submit_resources(
            CommandRunner::without_timeout(),
            Command::new("true").stdin_file("raw-command-input.txt"),
            CommandResourceRunOptions::new()
                .options(CommandRunOptions::new().tee_stdout_to_file("relative-output.log"))
                .stdin_file(input),
        ) {
            Ok(_) => panic!("submission must reject a missing working-directory snapshot"),
            Err(failure) => failure,
        };
        match &failure.reason {
            CommandSubmitFailureReason::Snapshot(snapshot) => {
                assert!(snapshot.diagnostic.contains("parent directory snapshot failed"));
            }
            other => panic!("expected snapshot failure, got {other:?}"),
        }

        let mut returned_input = failure
            .options
            .configured_stdin()
            .expect("rejected submission must return its stdin owner")
            .try_clone()
            .expect("returned stdin should remain usable");
        let mut bytes = Vec::new();
        returned_input
            .read_to_end(&mut bytes)
            .expect("returned stdin should retain its bytes");
        assert_eq!(bytes, b"recoverable input bytes");
        assert_eq!(failure.command.program(), std::ffi::OsStr::new("true"));
        assert_eq!(
            failure.command.stdin_file_path(),
            Some(std::path::Path::new("raw-command-input.txt"))
        );
        assert_eq!(
            failure.options.ordinary_options().configured_stdout_file(),
            Some(std::path::Path::new("relative-output.log"))
        );

        let standard_failure = match executor.submit(
            CommandRunner::without_timeout(),
            Command::new("must-not-run-after-standard-snapshot-failure"),
            CommandRunOptions::new().tee_stdout_to_file("relative-standard-output.log"),
        ) {
            Ok(_) => panic!("standard submission must reject a missing working-directory snapshot"),
            Err(failure) => failure,
        };
        assert_eq!(
            standard_failure.command.program(),
            "must-not-run-after-standard-snapshot-failure"
        );
        assert!(matches!(
            standard_failure.reason,
            CommandSubmitFailureReason::Snapshot(ref snapshot)
                if snapshot.diagnostic.contains("parent directory snapshot failed")
        ));

        let scoped_executor = executor.clone();
        let scoped_result = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let scoped_failure = match scoped_executor.submit_in(
                    &scope,
                    CommandRunner::without_timeout(),
                    Command::new("must-not-run-after-scoped-snapshot-failure"),
                    CommandRunOptions::new().tee_stdout_to_file("relative-scoped-output.log"),
                ) {
                    Ok(_) => panic!("scoped submission must reject a missing working-directory snapshot"),
                    Err(failure) => failure,
                };
                assert!(matches!(
                    scoped_failure.reason,
                    CommandSubmitFailureReason::Snapshot(ref snapshot)
                        if snapshot.diagnostic.contains("parent directory snapshot failed")
                ));

                let mut input = tempfile::tempfile().expect("scoped resource input should be created");
                input.write_all(b"scoped snapshot input").unwrap();
                input.rewind().unwrap();
                let scoped_resource_failure = match scoped_executor.submit_resources_in(
                    &scope,
                    CommandRunner::without_timeout(),
                    Command::new("must-not-run-after-scoped-resource-snapshot-failure"),
                    CommandResourceRunOptions::new()
                        .options(CommandRunOptions::new().tee_stdout_to_file("relative-scoped-resource-output.log"))
                        .stdin_file(input),
                ) {
                    Ok(_) => panic!("scoped resource submission must reject a missing working-directory snapshot"),
                    Err(failure) => failure,
                };
                assert!(matches!(
                    scoped_resource_failure.reason,
                    CommandSubmitFailureReason::Snapshot(ref snapshot)
                        if snapshot.diagnostic.contains("parent directory snapshot failed")
                ));
                let mut returned_input = scoped_resource_failure
                    .options
                    .configured_stdin()
                    .expect("scoped snapshot rejection must return its input owner")
                    .try_clone()
                    .unwrap();
                returned_input.rewind().unwrap();
                let mut bytes = Vec::new();
                returned_input.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"scoped snapshot input");

                #[cfg(feature = "async")]
                {
                    let async_scoped_failure = match scoped_executor.submit_async_in(
                        &scope,
                        CommandRunner::without_timeout(),
                        Command::new("must-not-run-after-async-scoped-snapshot-failure"),
                        CommandRunOptions::new().tee_stdout_to_file("relative-async-scoped-output.log"),
                    ) {
                        Ok(_) => panic!("async scoped submission must reject a missing snapshot"),
                        Err(failure) => failure,
                    };
                    assert!(matches!(
                        async_scoped_failure.reason,
                        CommandSubmitFailureReason::Snapshot(ref snapshot)
                            if snapshot.diagnostic.contains("parent directory snapshot failed")
                    ));

                    let mut input = tempfile::tempfile().expect("async scoped input should be created");
                    input.write_all(b"async scoped snapshot input").unwrap();
                    input.rewind().unwrap();
                    let async_resource_failure = match scoped_executor.submit_async_resources_in(
                        &scope,
                        CommandRunner::without_timeout(),
                        Command::new("must-not-run-after-async-scoped-resource-snapshot-failure"),
                        CommandResourceRunOptions::new()
                            .options(
                                CommandRunOptions::new()
                                    .tee_stdout_to_file("relative-async-scoped-resource-output.log"),
                            )
                            .stdin_file(input),
                    ) {
                        Ok(_) => panic!("async scoped resource submission must reject a missing snapshot"),
                        Err(failure) => failure,
                    };
                    assert!(matches!(
                        async_resource_failure.reason,
                        CommandSubmitFailureReason::Snapshot(ref snapshot)
                            if snapshot.diagnostic.contains("parent directory snapshot failed")
                    ));
                    let mut returned_input = async_resource_failure
                        .options
                        .configured_stdin()
                        .expect("async scoped snapshot rejection must return its input owner")
                        .try_clone()
                        .unwrap();
                    returned_input.rewind().unwrap();
                    let mut bytes = Vec::new();
                    returned_input.read_to_end(&mut bytes).unwrap();
                    assert_eq!(bytes, b"async scoped snapshot input");
                }
                Ok::<_, crate::CommandError>(())
            })
            .expect("snapshot rejections should not fail the parent operation");
        assert!(matches!(
            scoped_result,
            qubit_operation::OperationOutcome::Succeeded(())
        ));

        #[cfg(feature = "async")]
        {
            let async_failure = match executor.submit_async(
                CommandRunner::without_timeout(),
                Command::new("must-not-run-after-async-snapshot-failure"),
                CommandRunOptions::new().tee_stdout_to_file("relative-async-output.log"),
            ) {
                Ok(_) => panic!("async submission must reject a missing working-directory snapshot"),
                Err(failure) => failure,
            };
            assert_eq!(
                async_failure.command.program(),
                "must-not-run-after-async-snapshot-failure"
            );
            assert!(matches!(
                async_failure.reason,
                CommandSubmitFailureReason::Snapshot(ref snapshot)
                    if snapshot.diagnostic.contains("parent directory snapshot failed")
            ));

            let mut input = tempfile::tempfile().expect("async stdin file should be created");
            input.write_all(b"async recoverable input").unwrap();
            input.rewind().unwrap();
            let async_resource_failure = match executor.submit_async_resources(
                CommandRunner::without_timeout(),
                Command::new("must-not-run-after-async-resource-snapshot-failure"),
                CommandResourceRunOptions::new()
                    .options(CommandRunOptions::new().tee_stdout_to_file("relative-async-resource-output.log"))
                    .stdin_file(input),
            ) {
                Ok(_) => panic!("async resource submission must reject a missing working-directory snapshot"),
                Err(failure) => failure,
            };
            assert!(matches!(
                async_resource_failure.reason,
                CommandSubmitFailureReason::Snapshot(ref snapshot)
                    if snapshot.diagnostic.contains("parent directory snapshot failed")
            ));
            let mut returned_input = async_resource_failure
                .options
                .configured_stdin()
                .expect("async snapshot rejection must return its input owner")
                .try_clone()
                .unwrap();
            returned_input.rewind().unwrap();
            let mut bytes = Vec::new();
            returned_input.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"async recoverable input");
        }
    }
}

#[cfg(all(test, feature = "async"))]
fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::Wake;
    use std::task::Waker;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park();
    }
}

impl CommandExecutorApi for CommandExecutor {
    /// Performs the submit step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit(self, r, c, o)
    }
    /// Performs the submit resources step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit_resources(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_resources(self, r, c, o)
    }
    /// Performs the submit in step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit_in(self, s, r, c, o)
    }
    /// Performs the submit resources in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit_resources_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_resources_in(self, s, r, c, o)
    }
    /// Performs the submit wait blocking in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit_wait_blocking_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_blocking_in_impl(self, scope, runner, command, options)
    }
    /// Performs the submit wait blocking step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit_wait_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_blocking_impl(self, runner, command, options)
    }
    /// Performs the submit wait resources blocking step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit_wait_resources_blocking(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_resources_blocking_impl(self, runner, command, options)
    }
    /// Performs the submit wait resources blocking in step in the managed
    /// command execution lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    fn submit_wait_resources_blocking_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_resources_blocking_in_impl(
            self, scope, runner, command, options,
        )
    }
    /// Performs the submit wait step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    async fn submit_wait(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_impl(self, runner, command, options).await
    }
    /// Performs the submit wait resources step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    async fn submit_wait_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_resources_impl(self, runner, command, options).await
    }
    /// Performs the submit wait resources in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    async fn submit_wait_resources_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_resources_in_impl(self, scope, runner, command, options)
            .await
    }
    /// Performs the submit async step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit_async(self, r, c, o)
    }
    /// Performs the submit async resources step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async_resources(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_async_resources(self, r, c, o)
    }
    /// Performs the submit async in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit_async_in(self, s, r, c, o)
    }
    /// Performs the submit async resources in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    fn submit_async_resources_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_async_resources_in(self, s, r, c, o)
    }
    /// Performs the submit wait in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `s`: Supplies the s value used by this operation.
    /// * `r`: Supplies the r value used by this operation.
    /// * `c`: Supplies the c value used by this operation.
    /// * `o`: Supplies the o value used by this operation.
    #[cfg(feature = "async")]
    async fn submit_wait_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        super::command_executor::CommandExecutor::submit_wait_in_impl(self, scope, runner, command, options).await
    }
}
/// Performs the build executor step in the managed command execution lifecycle.
///
/// # Parameters
/// * `manager`: Supplies the manager value used by this operation.
/// * `config`: Supplies the config value used by this operation.
pub(crate) fn build_executor(
    manager: OperationManager,
    config: CommandExecutionConfig,
) -> Result<
    (
        BoundedScheduler,
        ProcessBackend,
        qubit_operation::OperationGroup,
        super::adapter::RecoverySender,
        OperationManager,
    ),
    CommandExecutorCreateError,
> {
    config.validate()?;
    let group = manager
        .group(GroupOptions {
            capacity: config.max_active.saturating_add(config.max_queued).max(1),
        })
        .map_err(|_| CommandExecutorCreateError::ZeroActiveCapacity)?;
    let manager_view = manager
        .with_group(&group)
        .map_err(CommandExecutorCreateError::GroupBinding)?;
    let scheduler = BoundedScheduler::new(config.clone())?;
    let backend = ProcessBackend::new();
    let worker_handles = spawn_domain_workers_with(&scheduler, config.max_active, |index, worker| {
        std::thread::Builder::new()
            .name(format!("qubit-command-worker-{index}"))
            .spawn(worker)
    })
    .map_err(|_| CommandExecutorCreateError::WorkerStartFailed)?;
    let recovery = match super::adapter::cleanup::start_recovery_worker(config.max_active) {
        Ok(recovery) => recovery,
        Err(_) => {
            scheduler.close_admission();
            for worker in worker_handles {
                let _ = worker.join();
            }
            return Err(CommandExecutorCreateError::WorkerStartFailed);
        }
    };
    drop(worker_handles);
    Ok((scheduler, backend, group, recovery, manager_view))
}

/// Performs the spawn domain workers with step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `scheduler`: Supplies the scheduler value used by this operation.
/// * `count`: Supplies the count value used by this operation.
/// * `spawn`: Supplies the spawn value used by this operation.
fn spawn_domain_workers_with(
    scheduler: &BoundedScheduler,
    count: usize,
    mut spawn: impl FnMut(usize, Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<std::thread::JoinHandle<()>>,
) -> std::io::Result<Vec<std::thread::JoinHandle<()>>> {
    let mut workers = Vec::with_capacity(count);
    for index in 0..count {
        let worker_scheduler = scheduler.clone();
        let worker: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
            while let Some((job, permit)) = worker_scheduler.wait_next() {
                (job.run)(permit);
            }
        });
        match spawn(index, worker) {
            Ok(handle) => workers.push(handle),
            Err(error) => {
                scheduler.close_admission();
                for worker in workers {
                    let _ = worker.join();
                }
                return Err(error);
            }
        }
    }
    Ok(workers)
}

#[cfg(all(test, feature = "async"))]
mod retry_wait_tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Waker;

    struct CountWake(AtomicUsize);

    impl std::task::Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::StopReason;
    use qubit_operation::TimerRegistration;

    use super::wait_capacity_async;
    use super::wait_retry_tick;
    use super::wait_retryable_async;
    use crate::CommandExecutionConfig;
    use crate::execution::contracts::Scheduler;
    use crate::execution::scheduler::BoundedScheduler;

    struct NoopRegistration;
    impl TimerRegistration for NoopRegistration {
        ///
        /// # Parameters
        /// * `self`: Supplies the self value used by this operation.
        fn cancel(self: Box<Self>) {}
    }
    struct ManualClock {
        domain: ClockDomainId,
        ticks: AtomicU64,
    }
    impl ClockSource for ManualClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.domain, self.ticks.load(Ordering::SeqCst))
        }
        ///
        /// # Parameters
        /// * `_`: Supplies the   value used by this operation.
        /// * `_`: Supplies the   value used by this operation.
        fn register_deadline(&self, _: MonotonicTime, _: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    #[test]
    fn test_retry_wait_observes_operation_cancellation() {
        let context = OperationContext::new(ContextOptions::default()).unwrap();
        let mut retry = Box::pin(wait_retry_tick(Some(context.clone())));
        let waker = Waker::noop().clone();
        let mut cx = Context::from_waker(&waker);
        assert!(retry.as_mut().poll(&mut cx).is_pending());
        assert!(context.request_stop(StopReason::UserCancelled));
        assert!(matches!(retry.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
    }

    #[test]
    fn test_retry_wait_observes_inherited_deadline() {
        let clock = Arc::new(ManualClock {
            domain: ClockDomainId::new_unique(),
            ticks: AtomicU64::new(0),
        });
        let source: Arc<dyn ClockSource> = clock.clone();
        let context = OperationContext::new(ContextOptions {
            clock: Some(source),
            deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 5))),
            budget: None,
        })
        .unwrap();
        let mut retry = Box::pin(wait_retry_tick(Some(context.clone())));
        let waker = Waker::noop().clone();
        let mut cx = Context::from_waker(&waker);
        assert!(retry.as_mut().poll(&mut cx).is_pending());
        clock.ticks.store(5, Ordering::SeqCst);
        assert!(matches!(retry.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
        assert_eq!(context.stop_reason(), Some(StopReason::Timeout));
    }

    #[test]
    fn test_capacity_wait_observes_stop_and_removes_its_waiter() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let context = OperationContext::new(ContextOptions::default()).unwrap();
        let reason =
            crate::execution::CommandSubmitFailureReason::Capacity(crate::execution::CapacityFailure::ActiveFull);
        let mut wait = Box::pin(wait_retryable_async(
            scheduler.clone(),
            0,
            &reason,
            Some(context.clone()),
        ));
        let mut cx = Context::from_waker(Waker::noop());

        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        assert!(context.request_stop(StopReason::UserCancelled));
        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
        assert_eq!(
            scheduler.snapshot().waiters,
            0,
            "cancellation must unregister capacity waiter"
        );
        drop(reservation);
    }

    #[test]
    fn test_scoped_resource_blocking_wait_returns_stopped_with_original_request_while_full() {
        use std::io::Read;
        use std::io::Seek;
        use std::io::Write;
        use std::path::Path;
        use std::time::Duration;
        use std::time::Instant;

        use qubit_operation::OperationManager;
        use qubit_operation::StopReason;

        use crate::Command;
        use crate::CommandExecutor;
        use crate::CommandResourceRunOptions;
        use crate::CommandRunner;
        use crate::CommandSubmitFailureReason;
        use crate::execution::CommandExecutorApi;
        use crate::execution::contracts::Scheduler;

        let manager = OperationManager::new(Default::default()).unwrap();
        let executor = CommandExecutor::new(
            manager.clone(),
            CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                max_waiters: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let reservation = executor.scheduler.try_reserve(0).unwrap();
        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"original scoped resource input").unwrap();
        input.rewind().unwrap();
        let (context_tx, context_rx) = std::sync::mpsc::channel();
        let (failure_tx, failure_rx) = std::sync::mpsc::channel();
        let waiting_executor = executor.clone();
        let submitter = std::thread::spawn(move || {
            let outcome = manager
                .run(Default::default(), move |scope| {
                    context_tx.send(scope.context()).unwrap();
                    let failure = match waiting_executor.submit_wait_resources_blocking_in(
                        &scope,
                        CommandRunner::without_timeout().working_directory("resource-retained-directory"),
                        Command::new("must-not-start-resource").arg("resource-original-arg"),
                        CommandResourceRunOptions::new().stdin_file(input),
                    ) {
                        Err(failure) => failure,
                        Ok(_) => panic!("scope stop should reject the waiting resource submission"),
                    };
                    failure_tx.send(failure).unwrap();
                    Ok::<_, ()>(())
                })
                .unwrap();
            drop(outcome);
        });
        let context = context_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let setup_deadline = Instant::now() + Duration::from_secs(2);
        while executor.scheduler.snapshot().waiters == 0 && Instant::now() < setup_deadline {
            std::thread::yield_now();
        }
        assert_eq!(executor.scheduler.snapshot().waiters, 1);
        assert!(context.request_stop(StopReason::UserCancelled));

        let failure = failure_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("scoped stop should return the original resource request promptly");
        assert_eq!(
            failure.reason,
            CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Stopped)
        );
        assert_eq!(
            failure.command.program(),
            std::ffi::OsStr::new("must-not-start-resource")
        );
        assert_eq!(
            failure.command.arguments().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("resource-original-arg")]
        );
        assert_eq!(
            failure.runner.configured_working_directory(),
            Some(Path::new("resource-retained-directory"))
        );
        let mut returned = failure.options.configured_stdin().unwrap().try_clone().unwrap();
        returned.rewind().unwrap();
        let mut contents = Vec::new();
        returned.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, b"original scoped resource input");
        assert_eq!(executor.scheduler.snapshot().waiters, 0);
        assert_eq!(executor.scheduler.snapshot().active, 1);
        drop(reservation);
        submitter.join().unwrap();
    }

    #[test]
    fn test_async_busy_wait_observes_waiter_limit_and_scheduler_close() {
        let busy = crate::execution::CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy);
        let full = BoundedScheduler::new(CommandExecutionConfig {
            max_waiters: 0,
            ..Default::default()
        })
        .unwrap();
        let mut rejected = Box::pin(wait_retryable_async(full.clone(), 0, &busy, None));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            rejected.as_mut().poll(&mut cx),
            Poll::Ready(Err(crate::execution::CommandSubmitWaitFailure::WaiterLimit))
        ));
        assert_eq!(full.snapshot().waiters, 0);

        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let mut waiting = Box::pin(wait_retryable_async(scheduler.clone(), 0, &busy, None));
        assert!(waiting.as_mut().poll(&mut cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        scheduler.close_admission();
        assert!(matches!(waiting.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
        assert_eq!(scheduler.snapshot().waiters, 0);
    }

    #[test]
    fn test_capacity_wait_refreshes_latest_waker_while_capacity_stays_full() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let first = Arc::new(CountWake(AtomicUsize::new(0)));
        let second = Arc::new(CountWake(AtomicUsize::new(0)));
        let first_waker = Waker::from(first.clone());
        let second_waker = Waker::from(second.clone());
        let mut first_cx = Context::from_waker(&first_waker);
        let mut second_cx = Context::from_waker(&second_waker);
        let mut wait = Box::pin(wait_capacity_async(scheduler.clone(), 0, None));

        assert!(wait.as_mut().poll(&mut first_cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        assert!(wait.as_mut().poll(&mut second_cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        drop(reservation);
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
        assert!(matches!(wait.as_mut().poll(&mut second_cx), Poll::Ready(Ok(true))));
        assert_eq!(scheduler.snapshot().waiters, 0);
    }

    #[test]
    fn test_capacity_wait_retries_after_reservation_release() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let mut wait = Box::pin(wait_capacity_async(scheduler.clone(), 0, None));
        let mut cx = Context::from_waker(Waker::noop());

        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        drop(reservation);

        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(true))));
        assert_eq!(scheduler.snapshot().waiters, 0);
        let retried = scheduler
            .try_reserve(0)
            .expect("released capacity should admit the waiting request");
        assert_eq!(scheduler.snapshot().active, 1);
        drop(retried);
        assert_eq!(scheduler.snapshot().active, 0);
    }

    #[test]
    fn test_capacity_wait_retries_after_release_before_deadline() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let clock = Arc::new(ManualClock {
            domain: ClockDomainId::new_unique(),
            ticks: AtomicU64::new(0),
        });
        let source: Arc<dyn ClockSource> = clock.clone();
        let context = OperationContext::new(ContextOptions {
            clock: Some(source),
            deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 5))),
            budget: None,
        })
        .unwrap();
        let mut wait = Box::pin(wait_capacity_async(scheduler.clone(), 0, Some(context.clone())));
        let mut cx = Context::from_waker(Waker::noop());

        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        drop(reservation);

        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(true))));
        assert_eq!(scheduler.snapshot().waiters, 0);
        assert_eq!(context.stop_reason(), None);
        let retried = scheduler
            .try_reserve(0)
            .expect("released capacity remains usable before the deadline");
        assert_eq!(scheduler.snapshot().active, 1);
        drop(retried);
    }

    #[test]
    fn test_capacity_wait_stop_and_deadline_win_simultaneous_capacity_release() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let context = OperationContext::new(ContextOptions::default()).unwrap();
        let mut wait = Box::pin(wait_capacity_async(scheduler.clone(), 0, Some(context.clone())));
        let mut cx = Context::from_waker(Waker::noop());

        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        assert!(context.request_stop(StopReason::UserCancelled));
        drop(reservation);

        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
        assert_eq!(scheduler.snapshot().waiters, 0);
        assert_eq!(scheduler.snapshot().active, 0);

        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 1,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let clock = Arc::new(ManualClock {
            domain: ClockDomainId::new_unique(),
            ticks: AtomicU64::new(0),
        });
        let source: Arc<dyn ClockSource> = clock.clone();
        let context = OperationContext::new(ContextOptions {
            clock: Some(source),
            deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 5))),
            budget: None,
        })
        .unwrap();
        let mut wait = Box::pin(wait_capacity_async(scheduler.clone(), 0, Some(context.clone())));

        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        clock.ticks.store(5, Ordering::SeqCst);
        drop(reservation);

        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
        assert_eq!(scheduler.snapshot().waiters, 0);
        assert_eq!(scheduler.snapshot().active, 0);
        assert_eq!(context.stop_reason(), Some(StopReason::Timeout));
    }

    #[test]
    fn test_capacity_wait_rejects_closed_scheduler_and_expired_context() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        scheduler.close_admission();
        let mut wait = Box::pin(wait_capacity_async(scheduler.clone(), 0, None));
        let mut cx = Context::from_waker(Waker::noop());

        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
        assert_eq!(scheduler.snapshot().waiters, 0);
        drop(reservation);

        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            ..Default::default()
        })
        .unwrap();
        let reservation = scheduler.try_reserve(0).unwrap();
        let clock = Arc::new(ManualClock {
            domain: ClockDomainId::new_unique(),
            ticks: AtomicU64::new(0),
        });
        let source: Arc<dyn ClockSource> = clock.clone();
        let context = OperationContext::new(ContextOptions {
            clock: Some(source),
            deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 5))),
            budget: None,
        })
        .unwrap();
        let mut wait = Box::pin(wait_capacity_async(scheduler.clone(), 0, Some(context)));
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(scheduler.snapshot().waiters, 1);
        clock.ticks.store(5, Ordering::SeqCst);
        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(false))));
        assert_eq!(
            scheduler.snapshot().waiters,
            0,
            "deadline must unregister capacity waiter"
        );
        drop(reservation);
    }
}

#[cfg(test)]
mod worker_start_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::thread;

    use super::spawn_domain_workers_with;
    use crate::CommandExecutionConfig;
    use crate::execution::scheduler::BoundedScheduler;

    #[test]
    fn test_partial_worker_spawn_failure_closes_and_joins_started_workers() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            ..Default::default()
        })
        .unwrap();
        let exited = Arc::new(AtomicUsize::new(0));
        let worker_exited = Arc::clone(&exited);
        let result = spawn_domain_workers_with(&scheduler, 2, move |index, worker| {
            if index == 1 {
                return Err(std::io::Error::other("injected worker startup failure"));
            }
            let worker_exited = Arc::clone(&worker_exited);
            thread::Builder::new().spawn(move || {
                worker();
                worker_exited.fetch_add(1, Ordering::SeqCst);
            })
        });

        assert!(result.is_err());
        assert_eq!(exited.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.snapshot().active, 0);
        assert_eq!(scheduler.snapshot().queued, 0);
    }
}

#[cfg(test)]
mod command_executor_api_tests {
    use std::time::Duration;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use crate::Command;
    use crate::CommandExecutionConfig;
    use crate::CommandExecutor;
    #[cfg(unix)]
    use crate::CommandExecutorApi;
    #[cfg(unix)]
    use crate::CommandResourceRunOptions;
    use crate::CommandRunner;

    fn setup() -> (OperationManager, CommandExecutor, CommandRunner) {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let executor =
            CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).expect("executor should start");
        (manager, executor, CommandRunner::new(Duration::from_secs(5)))
    }

    #[cfg(feature = "async")]
    #[test]
    fn test_session_channels_apply_runner_aggregate_output_limit() {
        use crate::session::contracts::SessionBridge;

        let configured = CommandRunner::without_timeout().max_total_output_bytes(7);
        let channels = super::session_channels(&configured);
        assert_eq!(channels.bridge.output_budget_limit(), Some(7));

        let unbounded = CommandRunner::without_timeout();
        let channels = super::session_channels(&unbounded);
        assert_eq!(channels.bridge.output_budget_limit(), None);
    }

    #[cfg(unix)]
    fn output_and_settle(mut operation: crate::CommandOperation) -> Vec<u8> {
        let output = match operation.wait_outcome().expect("command outcome should be observable") {
            OperationOutcome::Succeeded(output) => output,
            other => panic!("command did not succeed: {other:?}"),
        };
        assert!(
            operation
                .wait_settlement()
                .expect("command should settle")
                .all_released()
        );
        output.stdout().to_vec()
    }

    #[test]
    fn test_executor_snapshot_shutdown_and_foreign_scope_rejection() {
        let (_manager, executor, runner) = setup();
        assert_eq!(executor.scheduler_snapshot().active, 0);
        assert_eq!(executor.scheduler_snapshot().queued, 0);

        let other_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let other_executor = executor.clone();
        let result = other_manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                match other_executor.submit_in(
                    &scope,
                    runner,
                    Command::new("sh").arg("-c").arg("exit 0"),
                    Default::default(),
                ) {
                    Err(failure) => assert!(matches!(
                        failure.reason,
                        super::CommandSubmitFailureReason::GroupBinding(_)
                    )),
                    Ok(_) => panic!("a scope from another manager must be rejected"),
                }
                Ok::<_, crate::CommandError>(())
            })
            .unwrap();
        assert!(matches!(result, OperationOutcome::Succeeded(())));

        let _handle = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    }

    #[cfg(unix)]
    #[test]
    fn test_scoped_resource_submission_returns_stdin_when_scope_is_foreign() {
        use std::io::Read;
        use std::io::Seek;
        use std::io::SeekFrom;
        use std::io::Write;

        let (_manager, executor, runner) = setup();
        let foreign_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"foreign-scope-input").unwrap();
        input.rewind().unwrap();
        let marker_directory = tempfile::tempdir().unwrap();
        let marker = marker_directory.path().join("command-ran");
        let command_marker = marker.clone();
        let scoped_executor = executor.clone();

        let outcome = foreign_manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let failure = match scoped_executor.submit_resources_in(
                    &scope,
                    runner,
                    Command::new("sh")
                        .arg("-c")
                        .arg("cat >/dev/null; touch \"$1\"")
                        .arg("sh")
                        .arg(command_marker.to_string_lossy().as_ref()),
                    CommandResourceRunOptions::new().stdin_file(input),
                ) {
                    Err(failure) => failure,
                    Ok(_) => panic!("resource submission with a foreign scope must be rejected"),
                };

                assert!(matches!(
                    failure.reason,
                    super::CommandSubmitFailureReason::GroupBinding(_)
                ));
                let mut returned_input = failure
                    .options
                    .configured_stdin()
                    .expect("rejected submission must return its stdin file")
                    .try_clone()
                    .expect("returned stdin should remain cloneable");
                returned_input.seek(SeekFrom::Start(0)).unwrap();
                let mut bytes = Vec::new();
                returned_input.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"foreign-scope-input");
                Ok::<_, crate::CommandError>(())
            })
            .unwrap();

        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
        assert!(!marker.exists(), "rejected command must not execute");
        assert_eq!(executor.scheduler_snapshot().active, 0);
        assert_eq!(executor.scheduler_snapshot().queued, 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_submit_and_blocking_wait_variants_run_and_settle_commands() {
        let (manager, executor, runner) = setup();
        assert_eq!(
            output_and_settle(
                executor
                    .submit(
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf submit"),
                        Default::default()
                    )
                    .unwrap(),
            ),
            b"submit"
        );

        let mut input = tempfile::tempfile().unwrap();
        use std::io::Seek;
        use std::io::Write;
        input.write_all(b"submit-resource").unwrap();
        input.rewind().unwrap();
        assert_eq!(
            output_and_settle(
                executor
                    .submit_resources(
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("cat"),
                        CommandResourceRunOptions::new().stdin_file(input),
                    )
                    .unwrap(),
            ),
            b"submit-resource"
        );

        assert_eq!(
            output_and_settle(
                executor
                    .submit_wait_blocking(
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf wait"),
                        Default::default(),
                    )
                    .unwrap(),
            ),
            b"wait"
        );
        assert_eq!(
            output_and_settle(
                executor
                    .submit_wait_resources_blocking(
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf wait-resource"),
                        CommandResourceRunOptions::new(),
                    )
                    .unwrap(),
            ),
            b"wait-resource"
        );

        let scoped_executor = executor.clone();
        let scoped_runner = runner.clone();
        let result = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let direct = output_and_settle(
                    scoped_executor
                        .submit_in(
                            &scope,
                            scoped_runner.clone(),
                            Command::new("sh").arg("-c").arg("printf submit-in"),
                            Default::default(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped submission rejected".into(),
                                },
                                None,
                            )
                        })?,
                );
                let owned = output_and_settle(
                    scoped_executor
                        .submit_resources_in(
                            &scope,
                            scoped_runner.clone(),
                            Command::new("sh").arg("-c").arg("printf submit-resources-in"),
                            CommandResourceRunOptions::new(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_resources_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped resource submission rejected".into(),
                                },
                                None,
                            )
                        })?,
                );
                let waited = output_and_settle(
                    scoped_executor
                        .submit_wait_blocking_in(
                            &scope,
                            scoped_runner.clone(),
                            Command::new("sh").arg("-c").arg("printf wait-in"),
                            Default::default(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_wait_blocking_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped wait submission rejected".into(),
                                },
                                None,
                            )
                        })?,
                );
                let waited_owned = output_and_settle(
                    scoped_executor
                        .submit_wait_resources_blocking_in(
                            &scope,
                            scoped_runner,
                            Command::new("sh").arg("-c").arg("printf wait-resources-in"),
                            CommandResourceRunOptions::new(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_wait_resources_blocking_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped resource wait rejected".into(),
                                },
                                None,
                            )
                        })?,
                );
                Ok::<_, crate::CommandError>((direct, owned, waited, waited_owned))
            })
            .expect("parent operation should complete");
        match result {
            OperationOutcome::Succeeded((direct, owned, waited, waited_owned)) => {
                assert_eq!(direct, b"submit-in");
                assert_eq!(owned, b"submit-resources-in");
                assert_eq!(waited, b"wait-in");
                assert_eq!(waited_owned, b"wait-resources-in");
            }
            other => panic!("scoped submission failed: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_blocking_wait_apis_return_closed_capacity_for_root_and_scope() {
        use super::CapacityFailure;
        use super::CommandSubmitFailureReason;
        use crate::execution::contracts::Scheduler;

        let (manager, executor, runner) = setup();
        executor.scheduler.close_admission();

        let root_failure = match <CommandExecutor as CommandExecutorApi>::submit_wait_blocking(
            &executor,
            runner.clone(),
            Command::new("must-not-run-root-wait-after-shutdown"),
            Default::default(),
        ) {
            Err(failure) => failure,
            Ok(_) => panic!("root blocking wait must return after shutdown closes admission"),
        };
        assert_eq!(
            root_failure.reason,
            CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
        );
        assert_eq!(root_failure.command.program(), "must-not-run-root-wait-after-shutdown");

        let scoped_executor = executor.clone();
        let scoped_result = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let failure = match <CommandExecutor as CommandExecutorApi>::submit_wait_blocking_in(
                    &scoped_executor,
                    &scope,
                    runner,
                    Command::new("must-not-run-scoped-wait-after-shutdown"),
                    Default::default(),
                ) {
                    Err(failure) => failure,
                    Ok(_) => panic!("scoped blocking wait must return after shutdown closes admission"),
                };
                assert_eq!(
                    failure.reason,
                    CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
                );
                assert_eq!(failure.command.program(), "must-not-run-scoped-wait-after-shutdown");
                Ok::<_, crate::CommandError>(())
            })
            .unwrap();
        assert!(matches!(scoped_result, OperationOutcome::Succeeded(())));
        assert_eq!(executor.scheduler_snapshot().waiters, 0);
        let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_trait_submission_entry_points_return_closed_capacity_and_owned_input() {
        use std::io::Read;
        use std::io::Seek;
        use std::io::Write;

        use super::CapacityFailure;
        use super::CommandSubmitFailureReason;
        use crate::execution::contracts::Scheduler;

        fn rejected<T, E>(result: Result<T, E>, message: &str) -> E {
            match result {
                Err(error) => error,
                Ok(_) => panic!("{message}"),
            }
        }

        fn resource_input(failure: crate::execution::CommandResourceSubmitFailure, expected: &[u8]) {
            assert_eq!(
                failure.reason,
                CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
            );
            let mut input = failure
                .options
                .configured_stdin()
                .expect("closed admission must return stdin ownership")
                .try_clone()
                .unwrap();
            input.rewind().unwrap();
            let mut bytes = Vec::new();
            input.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, expected);
        }

        type Submit = fn(
            &CommandExecutor,
            crate::CommandRunner,
            Command,
            crate::CommandRunOptions,
        ) -> Result<crate::CommandOperation, crate::CommandSubmitFailure>;
        type SubmitResources = fn(
            &CommandExecutor,
            crate::CommandRunner,
            Command,
            CommandResourceRunOptions,
        ) -> Result<crate::CommandOperation, crate::CommandResourceSubmitFailure>;
        type SubmitIn = for<'scope> fn(
            &CommandExecutor,
            &'scope qubit_operation::OperationScope,
            crate::CommandRunner,
            Command,
            crate::CommandRunOptions,
        ) -> Result<crate::CommandOperation, crate::CommandSubmitFailure>;
        type SubmitResourcesIn = for<'scope> fn(
            &CommandExecutor,
            &'scope qubit_operation::OperationScope,
            crate::CommandRunner,
            Command,
            CommandResourceRunOptions,
        )
            -> Result<crate::CommandOperation, crate::CommandResourceSubmitFailure>;

        let submit: Submit = <CommandExecutor as CommandExecutorApi>::submit;
        let submit_resources: SubmitResources = <CommandExecutor as CommandExecutorApi>::submit_resources;
        let submit_wait: Submit = <CommandExecutor as CommandExecutorApi>::submit_wait_blocking;
        let submit_wait_resources: SubmitResources =
            <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking;
        let submit_in: SubmitIn = <CommandExecutor as CommandExecutorApi>::submit_in;
        let submit_resources_in: SubmitResourcesIn = <CommandExecutor as CommandExecutorApi>::submit_resources_in;
        let submit_wait_in: SubmitIn = <CommandExecutor as CommandExecutorApi>::submit_wait_blocking_in;
        let submit_wait_resources_in: SubmitResourcesIn =
            <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking_in;

        let (manager, executor, runner) = setup();
        executor.scheduler.close_admission();

        let root = rejected(
            submit(
                &executor,
                runner.clone(),
                Command::new("must-not-run-root-submit"),
                Default::default(),
            ),
            "closed root submit must return its capacity error",
        );
        assert_eq!(
            root.reason,
            CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
        );

        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"root resource").unwrap();
        input.rewind().unwrap();
        let root_resources = rejected(
            submit_resources(
                &executor,
                runner.clone(),
                Command::new("must-not-run-root-resource-submit"),
                CommandResourceRunOptions::new().stdin_file(input),
            ),
            "closed root resource submit must return its capacity error",
        );
        resource_input(root_resources, b"root resource");

        let root_wait = rejected(
            submit_wait(
                &executor,
                runner.clone(),
                Command::new("must-not-run-root-wait"),
                Default::default(),
            ),
            "closed root wait must return without waiting",
        );
        assert_eq!(
            root_wait.reason,
            CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
        );

        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"root wait resource").unwrap();
        input.rewind().unwrap();
        let root_wait_resources = rejected(
            submit_wait_resources(
                &executor,
                runner.clone(),
                Command::new("must-not-run-root-resource-wait"),
                CommandResourceRunOptions::new().stdin_file(input),
            ),
            "closed root resource wait must return ownership without waiting",
        );
        resource_input(root_wait_resources, b"root wait resource");

        let scoped_executor = executor.clone();
        let scoped_result = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let scoped = rejected(
                    submit_in(
                        &scoped_executor,
                        &scope,
                        runner.clone(),
                        Command::new("must-not-run-scoped-submit"),
                        Default::default(),
                    ),
                    "closed scoped submit must return its capacity error",
                );
                assert_eq!(
                    scoped.reason,
                    CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
                );

                let mut input = tempfile::tempfile().unwrap();
                input.write_all(b"scoped resource").unwrap();
                input.rewind().unwrap();
                let scoped_resources = rejected(
                    submit_resources_in(
                        &scoped_executor,
                        &scope,
                        runner.clone(),
                        Command::new("must-not-run-scoped-resource-submit"),
                        CommandResourceRunOptions::new().stdin_file(input),
                    ),
                    "closed scoped resource submit must return its capacity error",
                );
                resource_input(scoped_resources, b"scoped resource");

                let scoped_wait = rejected(
                    submit_wait_in(
                        &scoped_executor,
                        &scope,
                        runner.clone(),
                        Command::new("must-not-run-scoped-wait"),
                        Default::default(),
                    ),
                    "closed scoped wait must return without waiting",
                );
                assert_eq!(
                    scoped_wait.reason,
                    CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
                );

                let mut input = tempfile::tempfile().unwrap();
                input.write_all(b"scoped wait resource").unwrap();
                input.rewind().unwrap();
                let scoped_wait_resources = rejected(
                    submit_wait_resources_in(
                        &scoped_executor,
                        &scope,
                        runner,
                        Command::new("must-not-run-scoped-resource-wait"),
                        CommandResourceRunOptions::new().stdin_file(input),
                    ),
                    "closed scoped resource wait must return ownership without waiting",
                );
                resource_input(scoped_wait_resources, b"scoped wait resource");
                Ok::<_, crate::CommandError>(())
            })
            .unwrap();
        assert!(matches!(scoped_result, OperationOutcome::Succeeded(())));
        assert_eq!(executor.scheduler_snapshot().waiters, 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_blocking_resource_wait_rejects_over_budget_input_and_returns_file() {
        use std::io::Read;
        use std::io::Seek;
        use std::io::Write;

        use crate::execution::CapacityFailure;
        use crate::execution::CommandSubmitFailureReason;

        fn read_returned_input(failure: crate::execution::CommandResourceSubmitFailure) -> Vec<u8> {
            assert_eq!(
                failure.reason,
                CommandSubmitFailureReason::Capacity(CapacityFailure::InputBytesExceeded)
            );
            let mut input = failure
                .options
                .configured_stdin()
                .expect("rejected input owner should be returned")
                .try_clone()
                .unwrap();
            input.rewind().unwrap();
            let mut bytes = Vec::new();
            input.read_to_end(&mut bytes).unwrap();
            bytes
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = CommandExecutor::new(
            manager.clone(),
            CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                max_waiters: 1,
                max_queued_input_bytes: 2,
                ..Default::default()
            },
        )
        .unwrap();

        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"too-large").unwrap();
        input.rewind().unwrap();
        let failure = match <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking(
            &executor,
            CommandRunner::without_timeout(),
            Command::new("must-not-run-blocking-resource-wait"),
            CommandResourceRunOptions::new().stdin_file(input),
        ) {
            Err(failure) => failure,
            Ok(_) => panic!("over-budget input must be rejected without waiting"),
        };
        assert_eq!(read_returned_input(failure), b"too-large");

        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"too-large-scoped").unwrap();
        input.rewind().unwrap();
        let scoped_executor = executor.clone();
        let scoped_result = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let failure = match <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking_in(
                    &scoped_executor,
                    &scope,
                    CommandRunner::without_timeout(),
                    Command::new("must-not-run-scoped-resource-wait"),
                    CommandResourceRunOptions::new().stdin_file(input),
                ) {
                    Err(failure) => failure,
                    Ok(_) => panic!("over-budget scoped input must be rejected without waiting"),
                };
                assert_eq!(read_returned_input(failure), b"too-large-scoped");
                Ok::<_, crate::CommandError>(())
            })
            .unwrap();
        assert!(matches!(scoped_result, OperationOutcome::Succeeded(())));
        let snapshot = executor.scheduler_snapshot();
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.queued, 0);
        assert_eq!(snapshot.waiters, 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_trait_submit_entry_points_execute_root_and_scoped_commands() {
        let (manager, executor, runner) = setup();
        assert_eq!(
            output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-submit"),
                    Default::default(),
                )
                .unwrap(),
            ),
            b"trait-submit"
        );

        let mut input = tempfile::tempfile().unwrap();
        use std::io::Seek;
        use std::io::Write;
        input.write_all(b"trait-resource").unwrap();
        input.rewind().unwrap();
        assert_eq!(
            output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit_resources(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("cat"),
                    CommandResourceRunOptions::new().stdin_file(input),
                )
                .unwrap(),
            ),
            b"trait-resource"
        );

        assert_eq!(
            output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit_wait_blocking(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-wait"),
                    Default::default(),
                )
                .unwrap(),
            ),
            b"trait-wait"
        );

        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"trait-wait-resource").unwrap();
        input.rewind().unwrap();
        assert_eq!(
            output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("cat"),
                    CommandResourceRunOptions::new().stdin_file(input),
                )
                .unwrap(),
            ),
            b"trait-wait-resource"
        );

        let scoped_executor = executor.clone();
        let result = manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let direct = output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_in(
                        &scoped_executor,
                        &scope,
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf trait-submit-in"),
                        Default::default(),
                    )
                    .map_err(|_| {
                        crate::CommandError::from_reason(
                            "trait submit_in",
                            crate::CommandErrorReason::ManagedOperationFailed {
                                diagnostic: "scoped trait submission rejected".into(),
                            },
                            None,
                        )
                    })?,
                );
                let resources = output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_resources_in(
                        &scoped_executor,
                        &scope,
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf trait-resources-in"),
                        CommandResourceRunOptions::new(),
                    )
                    .map_err(|_| {
                        crate::CommandError::from_reason(
                            "trait submit_resources_in",
                            crate::CommandErrorReason::ManagedOperationFailed {
                                diagnostic: "scoped resource trait submission rejected".into(),
                            },
                            None,
                        )
                    })?,
                );
                let waited = output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_wait_blocking_in(
                        &scoped_executor,
                        &scope,
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf trait-wait-in"),
                        Default::default(),
                    )
                    .map_err(|_| {
                        crate::CommandError::from_reason(
                            "trait submit_wait_blocking_in",
                            crate::CommandErrorReason::ManagedOperationFailed {
                                diagnostic: "scoped trait wait rejected".into(),
                            },
                            None,
                        )
                    })?,
                );
                let waited_resources = output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking_in(
                        &scoped_executor,
                        &scope,
                        runner,
                        Command::new("sh").arg("-c").arg("printf trait-wait-resources-in"),
                        CommandResourceRunOptions::new(),
                    )
                    .map_err(|_| {
                        crate::CommandError::from_reason(
                            "trait submit_wait_resources_blocking_in",
                            crate::CommandErrorReason::ManagedOperationFailed {
                                diagnostic: "scoped trait resource wait rejected".into(),
                            },
                            None,
                        )
                    })?,
                );
                Ok::<_, crate::CommandError>((direct, resources, waited, waited_resources))
            })
            .expect("parent operation should complete");
        match result {
            OperationOutcome::Succeeded((direct, resources, waited, waited_resources)) => {
                assert_eq!(direct, b"trait-submit-in");
                assert_eq!(resources, b"trait-resources-in");
                assert_eq!(waited, b"trait-wait-in");
                assert_eq!(waited_resources, b"trait-wait-resources-in");
            }
            other => panic!("scoped trait submission failed: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_trait_wait_methods_and_foreign_scope_rejection() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let executor = CommandExecutor::new(
            manager.clone(),
            CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                ..Default::default()
            },
        )
        .unwrap();
        let runner = CommandRunner::new(Duration::from_secs(5));
        let capacity_executor = executor.clone();
        let capacity_runner = runner.clone();
        let occupy_capacity = move || {
            let operation = capacity_executor
                .submit(
                    capacity_runner.clone(),
                    Command::new("sh").arg("-c").arg("sleep 0.3"),
                    Default::default(),
                )
                .unwrap();
            let until = std::time::Instant::now() + Duration::from_secs(2);
            while capacity_executor.scheduler_snapshot().active == 0 {
                assert!(
                    std::time::Instant::now() < until,
                    "command worker did not acquire capacity"
                );
                std::thread::yield_now();
            }
            operation
        };
        let first = occupy_capacity();
        let first_waiter = std::thread::spawn(move || output_and_settle(first));
        assert_eq!(
            output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit_wait_blocking(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-wait"),
                    Default::default(),
                )
                .unwrap(),
            ),
            b"trait-wait"
        );
        assert!(first_waiter.join().unwrap().is_empty());
        let second = occupy_capacity();
        let second_waiter = std::thread::spawn(move || output_and_settle(second));
        assert_eq!(
            output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-wait-owned"),
                    CommandResourceRunOptions::new(),
                )
                .unwrap(),
            ),
            b"trait-wait-owned"
        );
        assert!(second_waiter.join().unwrap().is_empty());

        let scoped_executor = executor.clone();
        let scoped_runner = runner.clone();
        let scoped = manager.run(Default::default(), move |scope| {
            let ordinary = output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit_wait_blocking_in(
                    &scoped_executor,
                    &scope,
                    scoped_runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-scoped-wait"),
                    Default::default(),
                )
                .map_err(|_| {
                    crate::CommandError::from_reason(
                        "trait submit_wait_blocking_in",
                        crate::CommandErrorReason::ManagedOperationFailed {
                            diagnostic: "scoped wait rejected".into(),
                        },
                        None,
                    )
                })?,
            );
            let resources = output_and_settle(
                <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking_in(
                    &scoped_executor,
                    &scope,
                    scoped_runner,
                    Command::new("sh").arg("-c").arg("printf trait-scoped-resource-wait"),
                    CommandResourceRunOptions::new(),
                )
                .map_err(|_| {
                    crate::CommandError::from_reason(
                        "trait submit_wait_resources_blocking_in",
                        crate::CommandErrorReason::ManagedOperationFailed {
                            diagnostic: "scoped resource wait rejected".into(),
                        },
                        None,
                    )
                })?,
            );
            Ok::<_, crate::CommandError>((ordinary, resources))
        });
        match scoped.unwrap() {
            OperationOutcome::Succeeded((ordinary, resources)) => {
                assert_eq!(ordinary, b"trait-scoped-wait");
                assert_eq!(resources, b"trait-scoped-resource-wait");
            }
            other => panic!("scoped trait waits failed: {other:?}"),
        }

        let other_manager = OperationManager::new(Default::default()).unwrap();
        let result = other_manager.run(Default::default(), move |scope| {
            assert!(
                <CommandExecutor as CommandExecutorApi>::submit_in(
                    &executor,
                    &scope,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("true"),
                    Default::default(),
                )
                .is_err()
            );
            assert!(
                <CommandExecutor as CommandExecutorApi>::submit_resources_in(
                    &executor,
                    &scope,
                    runner,
                    Command::new("sh").arg("-c").arg("true"),
                    CommandResourceRunOptions::new(),
                )
                .is_err()
            );
            Ok::<_, crate::CommandError>(())
        });
        assert!(matches!(result.unwrap(), OperationOutcome::Succeeded(())));
    }

    #[cfg(all(feature = "async", unix))]
    mod async_tests {
        use std::future::Future;
        use std::io::Read;
        use std::io::Seek;
        use std::io::Write;
        use std::sync::Arc;
        use std::task::Context;
        use std::task::Poll;
        use std::task::Wake;
        use std::task::Waker;

        use super::Command;
        use super::CommandExecutionConfig;
        use super::CommandExecutor;
        use super::CommandExecutorApi;
        use super::CommandResourceRunOptions;
        use super::Duration;
        use super::OperationOutcome;
        use super::setup;
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

        async fn read_all(reader: &mut (impl qubit_io::AsyncInput<Item = u8> + Unpin)) -> std::io::Result<Vec<u8>> {
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

        async fn output_and_settle(mut operation: crate::AsyncCommandOperation) -> Vec<u8> {
            let output = match operation
                .wait_outcome()
                .await
                .expect("async outcome should be observable")
            {
                OperationOutcome::Succeeded(output) => output,
                other => panic!("async command did not succeed: {other:?}"),
            };
            assert!(operation.wait_settlement().await.all_released());
            output.stdout().to_vec()
        }

        #[test]
        fn test_async_submit_and_wait_variants_run_and_settle_commands() {
            block_on(async_submit_and_wait_variants_run_and_settle_commands_inner());
        }

        #[test]
        fn test_trait_async_entry_points_run_root_and_scoped_commands() {
            block_on(async_trait_entry_points_run_root_and_scoped_commands());
        }

        #[test]
        fn test_scoped_session_apis_reject_foreign_scope_before_admission() {
            block_on(async {
                use crate::CommandErrorKind;
                use crate::ManagedCommandRunnerApi;

                let (_manager, executor, runner) = setup();
                let foreign_manager = super::super::OperationManager::new(Default::default()).unwrap();
                let marker_directory = tempfile::tempdir().unwrap();
                let standard_marker = marker_directory.path().join("standard-session-ran");
                let resource_marker = marker_directory.path().join("resource-session-ran");
                let standard_command_marker = standard_marker.clone();
                let resource_command_marker = resource_marker.clone();
                let scoped_executor = executor.clone();
                let scoped_runner = runner.clone();
                let outcome = foreign_manager
                    .as_async()
                    .run(Default::default(), move |scope| async move {
                        let standard_error = match scoped_runner
                            .start_session_in(
                                &scope,
                                &scoped_executor,
                                Command::new("sh")
                                    .arg("-c")
                                    .arg("touch \"$1\"")
                                    .arg("sh")
                                    .arg(standard_command_marker.to_string_lossy().as_ref()),
                                Default::default(),
                            )
                            .await
                        {
                            Err(error) => error,
                            Ok(_) => panic!("a foreign scope must not start a managed session"),
                        };
                        assert_eq!(standard_error.kind(), CommandErrorKind::ManagedOperationFailed);
                        assert!(matches!(
                            standard_error.reason(),
                            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                                if diagnostic.starts_with("group binding failed:")
                        ));

                        let resource_error = match scoped_runner
                            .start_session_resources_in(
                                &scope,
                                &scoped_executor,
                                Command::new("sh")
                                    .arg("-c")
                                    .arg("touch \"$1\"")
                                    .arg("sh")
                                    .arg(resource_command_marker.to_string_lossy().as_ref()),
                                CommandResourceRunOptions::new(),
                            )
                            .await
                        {
                            Err(error) => error,
                            Ok(_) => panic!("a foreign scope must not start a resource session"),
                        };
                        assert_eq!(resource_error.kind(), CommandErrorKind::ManagedOperationFailed);
                        assert!(matches!(
                            resource_error.reason(),
                            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                                if diagnostic.starts_with("group binding failed:")
                        ));
                        Ok::<_, crate::CommandError>(())
                    })
                    .await
                    .unwrap();

                assert!(matches!(outcome, OperationOutcome::Succeeded(())));
                assert!(!standard_marker.exists());
                assert!(!resource_marker.exists());
                assert_eq!(executor.scheduler_snapshot().active, 0);
                assert_eq!(executor.scheduler_snapshot().queued, 0);
                assert_eq!(executor.scheduler_snapshot().waiters, 0);
            });
        }

        #[test]
        fn test_session_admission_failure_maps_standard_and_resource_errors() {
            block_on(async {
                use crate::CommandErrorKind;
                use crate::execution::contracts::Scheduler;

                let (manager, executor, runner) = setup();
                executor.scheduler.close_admission();

                let standard = match executor.start_session_managed(
                    runner.clone(),
                    Command::new("must-not-start-standard-session"),
                    Default::default(),
                ) {
                    Err(error) => error,
                    Ok(_) => panic!("closed scheduler must reject standard session admission"),
                };
                assert_eq!(standard.kind(), CommandErrorKind::ManagedOperationFailed);
                assert!(matches!(
                    standard.reason(),
                    crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                        if diagnostic.starts_with("managed session admission failed:")
                ));

                let resource = match executor.start_session_resources_managed(
                    runner.clone(),
                    Command::new("must-not-start-resource-session"),
                    CommandResourceRunOptions::new(),
                ) {
                    Err(error) => error,
                    Ok(_) => panic!("closed scheduler must reject resource session admission"),
                };
                assert_eq!(resource.kind(), CommandErrorKind::ManagedOperationFailed);
                assert!(matches!(
                    resource.reason(),
                    crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                        if diagnostic.starts_with("managed resource session admission failed:")
                ));

                let scoped_executor = executor.clone();
                let scoped = manager
                    .run(Default::default(), move |scope| {
                        let standard = match scoped_executor.start_session_managed_in(
                            &scope,
                            runner.clone(),
                            Command::new("must-not-start-scoped-standard-session"),
                            Default::default(),
                        ) {
                            Err(error) => error,
                            Ok(_) => panic!("closed scheduler must reject scoped standard session"),
                        };
                        assert!(matches!(
                            standard.reason(),
                            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                                if diagnostic.starts_with("managed session admission failed:")
                        ));

                        let resource = match scoped_executor.start_session_resources_managed_in(
                            &scope,
                            runner,
                            Command::new("must-not-start-scoped-resource-session"),
                            CommandResourceRunOptions::new(),
                        ) {
                            Err(error) => error,
                            Ok(_) => panic!("closed scheduler must reject scoped resource session"),
                        };
                        assert!(matches!(
                            resource.reason(),
                            crate::CommandErrorReason::ManagedOperationFailed { diagnostic }
                                if diagnostic.starts_with("managed resource session admission failed:")
                        ));
                        Ok::<_, crate::CommandError>(())
                    })
                    .unwrap();
                assert!(matches!(scoped, OperationOutcome::Succeeded(())));
                assert_eq!(executor.scheduler_snapshot().active, 0);
                assert_eq!(executor.scheduler_snapshot().waiters, 0);
            });
        }

        #[test]
        fn test_async_resource_wait_returns_over_budget_file_without_admission() {
            block_on(async {
                use crate::execution::CapacityFailure;
                use crate::execution::CommandSubmitFailureReason;

                let manager = super::super::OperationManager::new(Default::default()).unwrap();
                let executor = CommandExecutor::new(
                    manager,
                    CommandExecutionConfig {
                        max_active: 1,
                        max_queued: 0,
                        max_waiters: 1,
                        max_queued_input_bytes: 2,
                        ..Default::default()
                    },
                )
                .unwrap();
                let mut input = tempfile::tempfile().unwrap();
                input.write_all(b"over-budget").unwrap();
                input.rewind().unwrap();

                let failure = match <CommandExecutor as CommandExecutorApi>::submit_wait_resources(
                    &executor,
                    crate::CommandRunner::without_timeout(),
                    Command::new("must-not-run-over-budget"),
                    CommandResourceRunOptions::new().stdin_file(input),
                )
                .await
                {
                    Err(failure) => failure,
                    Ok(_) => panic!("an over-budget resource wait must be rejected"),
                };
                assert_eq!(
                    failure.reason,
                    CommandSubmitFailureReason::Capacity(CapacityFailure::InputBytesExceeded)
                );
                let mut returned_input = failure
                    .options
                    .configured_stdin()
                    .expect("rejected input owner should be returned")
                    .try_clone()
                    .unwrap();
                returned_input.rewind().unwrap();
                let mut returned_bytes = Vec::new();
                returned_input.read_to_end(&mut returned_bytes).unwrap();
                assert_eq!(returned_bytes, b"over-budget");
                let snapshot = executor.scheduler_snapshot();
                assert_eq!(snapshot.active, 0);
                assert_eq!(snapshot.queued, 0);
                assert_eq!(snapshot.waiters, 0);
            });
        }

        #[test]
        fn test_async_wait_apis_return_closed_capacity_for_root_and_scope() {
            block_on(async {
                use crate::execution::CapacityFailure;
                use crate::execution::CommandSubmitFailureReason;
                use crate::execution::contracts::Scheduler;

                let manager = super::super::OperationManager::new(Default::default()).unwrap();
                let executor = CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).unwrap();
                let runner = crate::CommandRunner::without_timeout();
                executor.scheduler.close_admission();

                let root_failure = match <CommandExecutor as CommandExecutorApi>::submit_wait(
                    &executor,
                    runner.clone(),
                    Command::new("must-not-run-root-async-wait-after-shutdown"),
                    Default::default(),
                )
                .await
                {
                    Err(failure) => failure,
                    Ok(_) => panic!("root async wait must return after shutdown closes admission"),
                };
                assert_eq!(
                    root_failure.reason,
                    CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
                );
                assert_eq!(
                    root_failure.command.program(),
                    "must-not-run-root-async-wait-after-shutdown"
                );

                let scoped_executor = executor.clone();
                let scoped_result = manager
                    .as_async()
                    .run(Default::default(), move |scope| async move {
                        let failure = match <CommandExecutor as CommandExecutorApi>::submit_wait_in(
                            &scoped_executor,
                            &scope,
                            runner,
                            Command::new("must-not-run-scoped-async-wait-after-shutdown"),
                            Default::default(),
                        )
                        .await
                        {
                            Err(failure) => failure,
                            Ok(_) => panic!("scoped async wait must return after shutdown closes admission"),
                        };
                        assert_eq!(
                            failure.reason,
                            CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
                        );
                        assert_eq!(
                            failure.command.program(),
                            "must-not-run-scoped-async-wait-after-shutdown"
                        );
                        Ok::<_, crate::CommandError>(())
                    })
                    .await
                    .unwrap();
                assert!(matches!(scoped_result, OperationOutcome::Succeeded(())));
                assert_eq!(executor.scheduler_snapshot().waiters, 0);
                let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
            });
        }

        #[test]
        fn test_async_wait_returns_capacity_error_when_waiter_is_closed() {
            use std::thread;
            use std::time::Instant;

            block_on(async {
                use crate::execution::CapacityFailure;
                use crate::execution::CommandSubmitFailureReason;
                use crate::execution::contracts::Scheduler;

                let manager = super::super::OperationManager::new(Default::default()).unwrap();
                let executor = CommandExecutor::new(
                    manager,
                    CommandExecutionConfig {
                        max_active: 1,
                        max_queued: 0,
                        max_waiters: 1,
                        ..Default::default()
                    },
                )
                .unwrap();
                let held_reservation = executor.scheduler.try_reserve(0).unwrap();
                assert_eq!(executor.scheduler_snapshot().active, 1);

                let waiting_executor = executor.clone();
                let waiter = thread::spawn(move || {
                    block_on(<CommandExecutor as CommandExecutorApi>::submit_wait(
                        &waiting_executor,
                        crate::CommandRunner::without_timeout(),
                        Command::new("must-not-run-after-waiter-close"),
                        Default::default(),
                    ))
                });
                let waiter_deadline = Instant::now() + Duration::from_secs(2);
                while executor.scheduler_snapshot().waiters == 0 {
                    assert!(
                        Instant::now() < waiter_deadline,
                        "async submission did not register its waiter"
                    );
                    thread::yield_now();
                }

                executor.scheduler.close_admission();
                let failure = match waiter.join().expect("async wait thread should finish") {
                    Err(failure) => failure,
                    Ok(_) => panic!("closed capacity must reject the waiting async submission"),
                };
                assert_eq!(
                    failure.reason,
                    CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
                );
                assert_eq!(failure.command.program(), "must-not-run-after-waiter-close");
                assert_eq!(executor.scheduler_snapshot().waiters, 0);
                assert_eq!(executor.scheduler_snapshot().active, 1);
                drop(held_reservation);
                assert_eq!(executor.scheduler_snapshot().active, 0);
                let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
            });
        }

        #[test]
        fn test_async_wait_returns_capacity_error_when_waiter_limit_is_zero() {
            block_on(async {
                use crate::execution::CommandSubmitFailureReason;

                let manager = super::super::OperationManager::new(Default::default()).unwrap();
                let executor = CommandExecutor::new(
                    manager,
                    CommandExecutionConfig {
                        max_active: 1,
                        max_queued: 0,
                        max_waiters: 0,
                        ..Default::default()
                    },
                )
                .unwrap();
                let mut active = executor
                    .submit(
                        crate::CommandRunner::without_timeout(),
                        Command::new("sh").arg("-c").arg("sleep 5"),
                        Default::default(),
                    )
                    .unwrap();
                let active_deadline = std::time::Instant::now() + Duration::from_secs(2);
                while executor.scheduler_snapshot().active == 0 {
                    assert!(
                        std::time::Instant::now() < active_deadline,
                        "command did not acquire the only slot"
                    );
                    std::thread::yield_now();
                }

                let failure = match <CommandExecutor as CommandExecutorApi>::submit_wait(
                    &executor,
                    crate::CommandRunner::without_timeout(),
                    Command::new("must-not-run-after-waiter-limit-rejection"),
                    Default::default(),
                )
                .await
                {
                    Err(failure) => failure,
                    Ok(_) => panic!("waiter budget zero must reject a saturated async wait"),
                };
                assert_eq!(
                    failure.reason,
                    CommandSubmitFailureReason::Wait(crate::execution::CommandSubmitWaitFailure::WaiterLimit)
                );
                assert_eq!(failure.command.program(), "must-not-run-after-waiter-limit-rejection");
                assert_eq!(executor.scheduler_snapshot().waiters, 0);

                let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
                assert!(matches!(active.wait_outcome().unwrap(), OperationOutcome::Stopped(_)));
                assert!(active.wait_settlement().unwrap().all_released());
            });
        }

        #[test]
        fn test_scoped_managed_sessions_settle_and_release_resource_admission() {
            block_on(async {
                let (manager, executor, runner) = setup();
                let scoped_executor = executor.clone();
                let scoped_runner = runner.clone();
                let result = manager
                    .as_async()
                    .run(Default::default(), move |scope| async move {
                        let mut standard = scoped_executor
                            .start_session_managed_in(
                                &scope,
                                scoped_runner.clone(),
                                Command::new("sh").arg("-c").arg("printf scoped-session"),
                                Default::default(),
                            )
                            .map_err(|error| {
                                crate::CommandError::from_reason(
                                    "start_session_managed_in",
                                    crate::CommandErrorReason::ManagedOperationFailed {
                                        diagnostic: error.to_string(),
                                    },
                                    None,
                                )
                            })?;
                        let mut resources = scoped_executor
                            .start_session_resources_managed_in(
                                &scope,
                                scoped_runner,
                                Command::new("sh").arg("-c").arg("printf scoped-resource-session"),
                                CommandResourceRunOptions::new(),
                            )
                            .map_err(|error| {
                                crate::CommandError::from_reason(
                                    "start_session_resources_managed_in",
                                    crate::CommandErrorReason::ManagedOperationFailed {
                                        diagnostic: error.to_string(),
                                    },
                                    None,
                                )
                            })?;
                        let mut standard_stdin = standard.take_stdin().expect("session stdin should be present");
                        qubit_io::AsyncClose::close_async(&mut standard_stdin)
                            .await
                            .expect("session stdin should close");
                        let mut standard_stdout = standard.take_stdout().expect("session stdout should be present");
                        let mut standard_stderr = standard.take_stderr().expect("session stderr should be present");
                        let standard_output = read_all(&mut standard_stdout)
                            .await
                            .expect("session stdout should be readable");
                        let standard_error = read_all(&mut standard_stderr)
                            .await
                            .expect("session stderr should be readable");
                        let mut standard_operation = standard
                            .take_operation()
                            .expect("standard session should own its operation");
                        assert!(matches!(
                            standard_operation
                                .wait_outcome()
                                .expect("session should have an outcome"),
                            OperationOutcome::Succeeded(_)
                        ));
                        standard_operation
                            .wait_settlement()
                            .expect("standard session should settle");

                        let mut resource_stdin = resources
                            .take_stdin()
                            .expect("resource session stdin should be present");
                        qubit_io::AsyncClose::close_async(&mut resource_stdin)
                            .await
                            .expect("resource session stdin should close");
                        let mut resource_stdout = resources
                            .take_stdout()
                            .expect("resource session stdout should be present");
                        let mut resource_stderr = resources
                            .take_stderr()
                            .expect("resource session stderr should be present");
                        let resource_output = read_all(&mut resource_stdout)
                            .await
                            .expect("resource session stdout should be readable");
                        let resource_error = read_all(&mut resource_stderr)
                            .await
                            .expect("resource session stderr should be readable");
                        let mut resource_operation = resources
                            .take_operation()
                            .expect("resource session should own its operation");
                        assert!(matches!(
                            resource_operation
                                .wait_outcome()
                                .expect("resource session should have an outcome"),
                            OperationOutcome::Succeeded(_)
                        ));
                        resource_operation
                            .wait_settlement()
                            .expect("resource session should settle");
                        Ok::<_, crate::CommandError>((standard_output, standard_error, resource_output, resource_error))
                    })
                    .await
                    .expect("both scoped sessions should complete");
                match result {
                    OperationOutcome::Succeeded((standard, standard_error, resource, resource_error)) => {
                        assert_eq!(standard, b"scoped-session");
                        assert!(standard_error.is_empty());
                        assert_eq!(resource, b"scoped-resource-session");
                        assert!(resource_error.is_empty());
                    }
                    _ => panic!("scoped sessions failed"),
                }
                assert_eq!(executor.scheduler_snapshot().active, 0);
                assert_eq!(executor.scheduler_snapshot().queued, 0);
            });
        }

        async fn async_trait_entry_points_run_root_and_scoped_commands() {
            let (manager, executor, runner) = setup();
            let ordinary = output_and_settle(
                <crate::CommandExecutor as CommandExecutorApi>::submit_async(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-async-submit"),
                    Default::default(),
                )
                .unwrap(),
            )
            .await;
            assert_eq!(ordinary, b"trait-async-submit");

            let resources = output_and_settle(
                <crate::CommandExecutor as CommandExecutorApi>::submit_async_resources(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-async-resources"),
                    CommandResourceRunOptions::new(),
                )
                .unwrap(),
            )
            .await;
            assert_eq!(resources, b"trait-async-resources");

            let waited = output_and_settle(
                <crate::CommandExecutor as CommandExecutorApi>::submit_wait(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-async-wait"),
                    Default::default(),
                )
                .await
                .unwrap(),
            )
            .await;
            assert_eq!(waited, b"trait-async-wait");

            let waited_resources = output_and_settle(
                <crate::CommandExecutor as CommandExecutorApi>::submit_wait_resources(
                    &executor,
                    runner.clone(),
                    Command::new("sh").arg("-c").arg("printf trait-async-wait-resources"),
                    CommandResourceRunOptions::new(),
                )
                .await
                .unwrap(),
            )
            .await;
            assert_eq!(waited_resources, b"trait-async-wait-resources");

            let scoped_executor = executor.clone();
            let result = manager
                .as_async()
                .run(Default::default(), move |scope| async move {
                    let direct = output_and_settle(
                        <crate::CommandExecutor as CommandExecutorApi>::submit_async_in(
                            &scoped_executor,
                            &scope,
                            runner.clone(),
                            Command::new("sh").arg("-c").arg("printf trait-async-in"),
                            Default::default(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "trait submit_async_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped async submission rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    let direct_resources = output_and_settle(
                        <crate::CommandExecutor as CommandExecutorApi>::submit_async_resources_in(
                            &scoped_executor,
                            &scope,
                            runner.clone(),
                            Command::new("sh").arg("-c").arg("printf trait-async-resources-in"),
                            CommandResourceRunOptions::new(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "trait submit_async_resources_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped async resource submission rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    let waited = output_and_settle(
                        <crate::CommandExecutor as CommandExecutorApi>::submit_wait_in(
                            &scoped_executor,
                            &scope,
                            runner.clone(),
                            Command::new("sh").arg("-c").arg("printf trait-async-wait-in"),
                            Default::default(),
                        )
                        .await
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "trait submit_wait_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped async wait rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    let waited_resources = output_and_settle(
                        <crate::CommandExecutor as CommandExecutorApi>::submit_wait_resources_in(
                            &scoped_executor,
                            &scope,
                            runner,
                            Command::new("sh").arg("-c").arg("printf trait-async-wait-resources-in"),
                            CommandResourceRunOptions::new(),
                        )
                        .await
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "trait submit_wait_resources_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "scoped async resource wait rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    Ok::<_, crate::CommandError>((direct, direct_resources, waited, waited_resources))
                })
                .await
                .expect("scoped trait submissions should complete");
            match result {
                OperationOutcome::Succeeded((direct, direct_resources, waited, waited_resources)) => {
                    assert_eq!(direct, b"trait-async-in");
                    assert_eq!(direct_resources, b"trait-async-resources-in");
                    assert_eq!(waited, b"trait-async-wait-in");
                    assert_eq!(waited_resources, b"trait-async-wait-resources-in");
                }
                other => panic!("scoped async trait submissions failed: {other:?}"),
            }
        }

        async fn async_submit_and_wait_variants_run_and_settle_commands_inner() {
            let (manager, executor, runner) = setup();
            assert_eq!(
                output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_async(
                        &executor,
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf async-submit"),
                        Default::default(),
                    )
                    .unwrap(),
                )
                .await,
                b"async-submit"
            );

            let mut input = tempfile::tempfile().unwrap();
            input.write_all(b"async-submit-resource").unwrap();
            input.rewind().unwrap();
            assert_eq!(
                output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_async_resources(
                        &executor,
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("cat"),
                        CommandResourceRunOptions::new().stdin_file(input),
                    )
                    .unwrap(),
                )
                .await,
                b"async-submit-resource"
            );

            assert_eq!(
                output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_wait(
                        &executor,
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("printf async-wait"),
                        Default::default()
                    )
                    .await
                    .unwrap(),
                )
                .await,
                b"async-wait"
            );

            let mut input = tempfile::tempfile().unwrap();
            input.write_all(b"async-wait-resource").unwrap();
            input.rewind().unwrap();
            assert_eq!(
                output_and_settle(
                    <CommandExecutor as CommandExecutorApi>::submit_wait_resources(
                        &executor,
                        runner.clone(),
                        Command::new("sh").arg("-c").arg("cat"),
                        CommandResourceRunOptions::new().stdin_file(input),
                    )
                    .await
                    .unwrap(),
                )
                .await,
                b"async-wait-resource"
            );

            let scoped_executor = executor.clone();
            let scoped_runner = runner.clone();
            let result = manager
                .as_async()
                .run(qubit_operation::OperationOptions::default(), move |scope| async move {
                    let direct = output_and_settle(
                        <CommandExecutor as CommandExecutorApi>::submit_async_in(
                            &scoped_executor,
                            &scope,
                            scoped_runner.clone(),
                            Command::new("sh").arg("-c").arg("printf async-in"),
                            Default::default(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_async_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "async scoped submission rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    let direct_owned = output_and_settle(
                        <CommandExecutor as CommandExecutorApi>::submit_async_resources_in(
                            &scoped_executor,
                            &scope,
                            scoped_runner.clone(),
                            Command::new("sh").arg("-c").arg("printf async-resources-in"),
                            CommandResourceRunOptions::new(),
                        )
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_async_resources_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "async scoped resource submission rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    let waited = output_and_settle(
                        <CommandExecutor as CommandExecutorApi>::submit_wait_in(
                            &scoped_executor,
                            &scope,
                            scoped_runner.clone(),
                            Command::new("sh").arg("-c").arg("printf async-wait-in"),
                            Default::default(),
                        )
                        .await
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_wait_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "async scoped wait rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    let mut input = tempfile::tempfile().expect("scoped async input should be created");
                    input.write_all(b"async-wait-resources-in").unwrap();
                    input.rewind().unwrap();
                    let waited_owned = output_and_settle(
                        <CommandExecutor as CommandExecutorApi>::submit_wait_resources_in(
                            &scoped_executor,
                            &scope,
                            scoped_runner,
                            Command::new("sh").arg("-c").arg("cat"),
                            CommandResourceRunOptions::new().stdin_file(input),
                        )
                        .await
                        .map_err(|_| {
                            crate::CommandError::from_reason(
                                "submit_wait_resources_in",
                                crate::CommandErrorReason::ManagedOperationFailed {
                                    diagnostic: "async scoped resource wait rejected".into(),
                                },
                                None,
                            )
                        })?,
                    )
                    .await;
                    Ok::<_, crate::CommandError>((direct, direct_owned, waited, waited_owned))
                })
                .await
                .expect("async parent operation should complete");
            match result {
                OperationOutcome::Succeeded((direct, direct_owned, waited, waited_owned)) => {
                    assert_eq!(direct, b"async-in");
                    assert_eq!(direct_owned, b"async-resources-in");
                    assert_eq!(waited, b"async-wait-in");
                    assert_eq!(waited_owned, b"async-wait-resources-in");
                }
                other => panic!("async scoped submission failed: {other:?}"),
            }
        }
    }
}
