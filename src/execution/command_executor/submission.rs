// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_operation::OperationContext;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationScope;

use super::super::CommandOperation;
use super::super::CommandResourceSubmitFailure;
use super::super::CommandSubmitFailure;
use super::super::CommandSubmitFailureReason;
use super::super::SnapshotFailure;
use super::super::adapter::AdapterState;
#[cfg(feature = "async")]
use super::super::command_executor::internal::async_submission_context::AsyncSubmissionContext;
use super::super::contracts::Request;
use super::super::contracts::RequestOptions;
use crate::Command;
#[cfg(feature = "async")]
use crate::CommandError;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
use crate::CommandRunner;
#[cfg(feature = "async")]
use crate::execution::AsyncCommandOperation;

#[cfg(feature = "async")]
use super::super::executor::managed_failure;
use super::super::executor::{make_request, resource_bytes};
use super::CommandExecutor;

/// Stores the session channels value used by this execution component.
///
/// # Parameters
/// * `runner`: Supplies the runner value used by this operation.
#[cfg(feature = "async")]
pub(in crate::execution) fn session_channels(
    runner: &CommandRunner,
) -> crate::session::channel::SessionChannels {
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
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure> {
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
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure> {
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
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure> {
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
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure> {
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
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure> {
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
        self.submit_resources_manager(
            &manager,
            runner,
            command,
            options,
            bytes,
            Some(scope.context()),
        )
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
        let request =
            make_request(runner, command, RequestOptions::Standard(options)).map_err(|error| {
                let (runner, command, options, message) = *error;
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
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
                Ok(CommandOperation::new(
                    handle,
                    prepared.stop_diagnostic_slot(),
                ))
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("unactivated submission retains request");
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
        let request =
            make_request(runner, command, RequestOptions::Resources(options)).map_err(|error| {
                let (runner, command, options, message) = *error;
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
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
                Ok(CommandOperation::new(
                    handle,
                    prepared.stop_diagnostic_slot(),
                ))
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("unactivated submission retains request");
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
    ) -> Result<AsyncCommandOperation, CommandSubmitFailure> {
        let AsyncSubmissionContext {
            bridge,
            keepalive,
            deadline,
        } = context;
        let RequestOptions::Standard(options) = request_options else {
            unreachable!()
        };
        let request =
            make_request(runner, command, RequestOptions::Standard(options)).map_err(|error| {
                let (runner, command, options, message) = *error;
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
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
                Ok(AsyncCommandOperation::new(
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
        self.start_session_managed_in_manager(
            &manager,
            runner,
            command,
            options,
            Some(scope.context()),
        )
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
        self.start_session_resources_managed_in_manager(
            &self.manager,
            runner,
            command,
            options,
            None,
        )
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
        self.start_session_resources_managed_in_manager(
            &manager,
            runner,
            command,
            options,
            Some(scope.context()),
        )
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
            .map_err(|e| {
                managed_failure(format!("managed session admission failed: {:?}", e.reason))
            })?;
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
            .map_err(|e| {
                managed_failure(format!(
                    "managed resource session admission failed: {:?}",
                    e.reason
                ))
            })?;
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
    ) -> Result<AsyncCommandOperation, CommandResourceSubmitFailure> {
        let AsyncSubmissionContext {
            bridge,
            keepalive,
            deadline,
        } = context;
        let request =
            make_request(runner, command, RequestOptions::Resources(options)).map_err(|error| {
                let (runner, command, options, message) = *error;
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
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
                Ok(AsyncCommandOperation::new(
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
