// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

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
use super::SnapshotFailure;
use super::adapter::AdapterState;
use super::backend::ProcessBackend;
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

/// Represents the async submission context state or contract used by managed
/// command execution.
#[cfg(feature = "async")]
struct AsyncSubmissionContext {
    /// Stores the bridge value used by this execution component.
    bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    /// Stores the keepalive value used by this execution component.
    keepalive: Option<super::adapter::SessionOwners>,
    /// Stores the deadline value used by this execution component.
    deadline: Option<OperationContext>,
}

#[cfg(feature = "async")]
impl AsyncSubmissionContext {
    /// Stores the plain value used by this execution component.
    ///
    /// # Parameters
    /// * `deadline`: Supplies the deadline value used by this operation.
    fn plain(deadline: Option<OperationContext>) -> Self {
        Self {
            bridge: None,
            keepalive: None,
            deadline,
        }
    }
}

impl CommandExecutor {
    /// Stores the scheduler snapshot value used by this execution component.
    pub fn scheduler_snapshot(&self) -> super::scheduler::SchedulerSnapshot {
        self.scheduler.snapshot()
    }
    /// Stops new local reservations and returns a manager handle for observing
    /// group shutdown.
    ///
    /// # Parameters
    /// * `mode`: Supplies the mode value used by this operation.
    pub fn request_shutdown(&self, mode: qubit_operation::ShutdownMode) -> qubit_operation::ShutdownHandle {
        self.scheduler.close_admission();
        self.group.request_shutdown(mode)
    }
    /// Performs the submit step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    pub fn submit(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        self.submit_in_manager(&self.manager, runner, command, options, None)
    }
    /// Performs the submit resources step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    pub fn submit_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        self.submit_resources_manager(&self.manager, runner, command, options, None)
    }
    /// Performs the submit async step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
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
    /// Performs the submit async resources step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    pub fn submit_async_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        self.submit_async_resources_manager(
            &self.manager,
            runner,
            command,
            options,
            AsyncSubmissionContext::plain(None),
        )
    }
    /// Performs the submit async in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
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
    /// Performs the submit async resources in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    pub fn submit_async_resources_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
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
            AsyncSubmissionContext::plain(Some(scope.context())),
        )
    }
    /// Performs the submit in step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
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
    /// Performs the submit resources in step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    pub fn submit_resources_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
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
        self.submit_resources_manager(&manager, runner, command, options, Some(scope.context()))
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
                Ok(handle)
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
    fn submit_resources_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
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
        let bytes = resource_bytes(&request.options);
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
                Ok(handle)
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
                Ok(handle)
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
    /// Performs the start session managed step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    pub(crate) fn start_session_managed(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        self.start_session_managed_in_manager(&self.manager, runner, command, options, None)
    }
    /// Performs the start session managed in step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
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
    /// Performs the start session resources managed step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
    #[cfg(feature = "async")]
    pub(crate) fn start_session_resources_managed(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        self.start_session_resources_managed_in_manager(&self.manager, runner, command, options, None)
    }
    /// Performs the start session resources managed in step in the managed
    /// command execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `runner`: Supplies the runner value used by this operation.
    /// * `command`: Supplies the command value used by this operation.
    /// * `options`: Supplies the options value used by this operation.
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
        let operation = self
            .submit_async_resources_manager(
                manager,
                runner,
                command,
                options,
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
    fn submit_async_resources_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
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
        let bytes = resource_bytes(&request.options);
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
                Ok(handle)
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
/// Stores the resource bytes value used by this execution component.
///
/// # Parameters
/// * `options`: Supplies the options value used by this operation.
fn resource_bytes(options: &RequestOptions) -> usize {
    match options {
        RequestOptions::Standard(_) => 0,
        RequestOptions::Resources(o) => [
            o.configured_stdin().and_then(|f| f.metadata().ok()).map(|m| m.len()),
            o.configured_stdout()
                .and_then(|(f, _)| f.metadata().ok())
                .map(|m| m.len()),
            o.configured_stderr()
                .and_then(|(f, _)| f.metadata().ok())
                .map(|m| m.len()),
        ]
        .into_iter()
        .flatten()
        .fold(0usize, |sum, n| {
            sum.saturating_add(usize::try_from(n).unwrap_or(usize::MAX))
        }),
    }
}
/// Stores the resource option bytes value used by this execution component.
///
/// # Parameters
/// * `options`: Supplies the options value used by this operation.
fn resource_option_bytes(options: &CommandResourceRunOptions) -> usize {
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
/// Performs the retry wait step in the managed command execution lifecycle.
///
/// # Parameters
/// * `reason`: Supplies the reason value used by this operation.
/// * `scheduler`: Supplies the scheduler value used by this operation.
/// * `bytes`: Supplies the bytes value used by this operation.
/// * `deadline`: Supplies the deadline value used by this operation.
fn retry_wait(
    reason: &CommandSubmitFailureReason,
    scheduler: &BoundedScheduler,
    bytes: usize,
    deadline: Option<OperationContext>,
) -> bool {
    match reason {
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull) => {
            scheduler.wait_for_capacity(bytes, deadline)
        }
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy) => {
            if deadline.as_ref().is_some_and(OperationContext::is_expired) {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            !deadline.as_ref().is_some_and(OperationContext::is_expired)
        }
        _ => false,
    }
}
/// Performs the deadline failure step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `reason`: Supplies the reason value used by this operation.
/// * `deadline`: Supplies the deadline value used by this operation.
fn deadline_failure(reason: CommandSubmitFailureReason, deadline: &OperationContext) -> CommandSubmitFailureReason {
    if deadline.is_expired() {
        CommandSubmitFailureReason::Capacity(CapacityFailure::DeadlineExpired)
    } else {
        reason
    }
}
/// Stores the async capacity retry value used by this execution component.
///
/// # Parameters
/// * `reason`: Supplies the reason value used by this operation.
#[cfg(feature = "async")]
fn async_capacity_retry(reason: &CommandSubmitFailureReason) -> bool {
    matches!(
        reason,
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull)
    )
}
/// Stores the retryable async value used by this execution component.
///
/// # Parameters
/// * `reason`: Supplies the reason value used by this operation.
#[cfg(feature = "async")]
fn retryable_async(reason: &CommandSubmitFailureReason) -> bool {
    async_capacity_retry(reason)
        || matches!(
            reason,
            CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy)
        )
}
/// Represents the capacity wait state or contract used by managed command
/// execution.
#[cfg(feature = "async")]
struct CapacityWait {
    /// Stores the scheduler value used by this execution component.
    scheduler: BoundedScheduler,
    /// Stores the bytes value used by this execution component.
    bytes: usize,
    /// Stores the waiter value used by this execution component.
    waiter: Option<u64>,
}
#[cfg(feature = "async")]
impl std::future::Future for CapacityWait {
    /// Represents the output state or contract used by managed command
    /// execution.
    type Output = bool;
    /// Performs the poll step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    /// * `cx`: Supplies the cx value used by this operation.
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<bool> {
        if let Some(id) = self.waiter.take() {
            self.scheduler.cancel_waiter(id);
            return std::task::Poll::Ready(true);
        }
        match self.scheduler.register_waiter_if_full(self.bytes, cx.waker()) {
            Ok(None) => std::task::Poll::Ready(true),
            Ok(Some(id)) => {
                self.waiter = Some(id);
                std::task::Poll::Pending
            }
            Err(_) => std::task::Poll::Ready(false),
        }
    }
}
#[cfg(feature = "async")]
impl Drop for CapacityWait {
    /// Performs the drop step in the managed command execution lifecycle.
    fn drop(&mut self) {
        if let Some(id) = self.waiter.take() {
            self.scheduler.cancel_waiter(id)
        }
    }
}
/// Performs the wait capacity async step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `scheduler`: Supplies the scheduler value used by this operation.
/// * `bytes`: Supplies the bytes value used by this operation.
/// * `context`: Supplies the context value used by this operation.
#[cfg(feature = "async")]
async fn wait_capacity_async(scheduler: BoundedScheduler, bytes: usize, context: Option<OperationContext>) -> bool {
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
                return std::task::Poll::Ready(false);
            }
            if deadline
                .as_mut()
                .is_some_and(|signal| signal.as_mut().poll(cx).is_ready())
            {
                return std::task::Poll::Ready(false);
            }
            if let std::task::Poll::Ready(value) = capacity.as_mut().poll(cx) {
                return std::task::Poll::Ready(value);
            }
            std::task::Poll::Pending
        })
        .await
    } else {
        capacity.await
    }
}

/// Performs the wait retryable async step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `scheduler`: Supplies the scheduler value used by this operation.
/// * `bytes`: Supplies the bytes value used by this operation.
/// * `reason`: Supplies the reason value used by this operation.
/// * `context`: Supplies the context value used by this operation.
#[cfg(feature = "async")]
async fn wait_retryable_async(
    scheduler: BoundedScheduler,
    bytes: usize,
    reason: &CommandSubmitFailureReason,
    context: Option<OperationContext>,
) -> bool {
    if async_capacity_retry(reason) {
        wait_capacity_async(scheduler, bytes, context).await
    } else if matches!(
        reason,
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy)
    ) {
        wait_retry_tick(context).await
    } else {
        false
    }
}

/// Represents the retry timer message state or contract used by managed command
/// execution.
#[cfg(feature = "async")]
struct RetryTimerMessage {
    /// Stores the due value used by this execution component.
    due: std::time::Instant,
    /// Stores the waker value used by this execution component.
    waker: std::task::Waker,
    /// Stores the ready value used by this execution component.
    ready: Arc<std::sync::atomic::AtomicBool>,
    /// Stores the cancelled value used by this execution component.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(feature = "async")]
static RETRY_TIMER: std::sync::OnceLock<Option<std::sync::mpsc::SyncSender<RetryTimerMessage>>> =
    std::sync::OnceLock::new();
/// Stores the retry timer value used by this execution component.
#[cfg(feature = "async")]
fn retry_timer() -> Option<&'static std::sync::mpsc::SyncSender<RetryTimerMessage>> {
    RETRY_TIMER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<RetryTimerMessage>(1024);
            std::thread::Builder::new()
                .name("qubit-command-admission-timer".to_owned())
                .spawn(move || {
                    let mut pending = Vec::<RetryTimerMessage>::new();
                    loop {
                        while let Ok(item) = rx.try_recv() {
                            pending.push(item)
                        }
                        let now = std::time::Instant::now();
                        let mut index = 0;
                        while index < pending.len() {
                            if pending[index].cancelled.load(std::sync::atomic::Ordering::Acquire) {
                                pending.swap_remove(index);
                            } else if pending[index].due <= now {
                                let item = pending.swap_remove(index);
                                item.ready.store(true, std::sync::atomic::Ordering::Release);
                                item.waker.wake()
                            } else {
                                index += 1
                            }
                        }
                        let wait = pending
                            .iter()
                            .map(|x| x.due.saturating_duration_since(std::time::Instant::now()))
                            .min()
                            .unwrap_or(std::time::Duration::from_secs(30));
                        if let Ok(item) = rx.recv_timeout(wait) {
                            pending.push(item)
                        }
                    }
                })
                .ok()?;
            Some(tx)
        })
        .as_ref()
}
/// Represents the retry delay state or contract used by managed command
/// execution.
#[cfg(feature = "async")]
struct RetryDelay {
    /// Stores the ready value used by this execution component.
    ready: Arc<std::sync::atomic::AtomicBool>,
    /// Stores the cancelled value used by this execution component.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    /// Stores the scheduled value used by this execution component.
    scheduled: bool,
}
#[cfg(feature = "async")]
impl RetryDelay {
    /// Performs the new step in the managed command execution lifecycle.
    fn new() -> Self {
        Self {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            scheduled: false,
        }
    }
}
#[cfg(feature = "async")]
impl std::future::Future for RetryDelay {
    /// Represents the output state or contract used by managed command
    /// execution.
    type Output = bool;
    /// Performs the poll step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    /// * `cx`: Supplies the cx value used by this operation.
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<bool> {
        if self.ready.load(std::sync::atomic::Ordering::Acquire) {
            return std::task::Poll::Ready(true);
        }
        if self.scheduled {
            return std::task::Poll::Pending;
        }
        let Some(tx) = retry_timer() else {
            return std::task::Poll::Ready(false);
        };
        let item = RetryTimerMessage {
            due: std::time::Instant::now() + std::time::Duration::from_millis(10),
            waker: cx.waker().clone(),
            ready: Arc::clone(&self.ready),
            cancelled: Arc::clone(&self.cancelled),
        };
        match tx.try_send(item) {
            Ok(()) => {
                self.scheduled = true;
                std::task::Poll::Pending
            }
            Err(_) => std::task::Poll::Ready(false),
        }
    }
}
#[cfg(feature = "async")]
impl Drop for RetryDelay {
    /// Performs the drop step in the managed command execution lifecycle.
    fn drop(&mut self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Release)
    }
}
/// Stores the wait retry tick value used by this execution component.
///
/// # Parameters
/// * `context`: Supplies the context value used by this operation.
#[cfg(feature = "async")]
async fn wait_retry_tick(context: Option<OperationContext>) -> bool {
    let mut delay = Box::pin(RetryDelay::new());
    if let Some(context) = context {
        let mut stop = Box::pin(context.stop_requested());
        let mut deadline = context.deadline_signal().map(Box::pin);
        std::future::poll_fn(|cx| {
            if stop.as_mut().poll(cx).is_ready() {
                return std::task::Poll::Ready(false);
            }
            if deadline
                .as_mut()
                .is_some_and(|signal| signal.as_mut().poll(cx).is_ready())
            {
                return std::task::Poll::Ready(false);
            }
            if let std::task::Poll::Ready(value) = delay.as_mut().poll(cx) {
                return std::task::Poll::Ready(value);
            }
            std::task::Poll::Pending
        })
        .await
    } else {
        delay.await
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
    let wd = if let Some(path) = runner
        .configured_working_directory()
        .or_else(|| command.working_directory_override())
    {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(path) => path,
            Err(e) => {
                return Err(Box::new((
                    runner,
                    command,
                    options,
                    format!("working directory snapshot failed: {e}"),
                )));
            }
        }
    };
    let env = std::env::vars_os().map(|(k, v)| (k, Some(v))).collect();
    let paths = match &options {
        RequestOptions::Standard(o) => IoPathSnapshot {
            stdin: None,
            stdout: o.configured_stdout_file().map(ToOwned::to_owned),
            stderr: o.configured_stderr_file().map(ToOwned::to_owned),
        },
        RequestOptions::Resources(_) => IoPathSnapshot::default(),
    };
    Ok(Request {
        operation_id: None,
        runner,
        command,
        options,
        snapshot: ExecutionSnapshot {
            working_directory: wd,
            environment: env,
        },
        io_paths: paths,
        deadline: None,
    })
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
        s: &OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        let deadline = s.context();
        loop {
            match self.submit_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, 0, Some(deadline.clone())) {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &deadline),
                    });
                }
            }
        }
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
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        loop {
            match self.submit(r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, 0, None) {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
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
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_resources(r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, bytes, None) {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
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
        s: &OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let deadline = s.context();
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_resources_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, bytes, Some(deadline.clone())) {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &deadline),
                    });
                }
            }
        }
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
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        loop {
            match self.submit_async(r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), 0, &reason, None).await {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
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
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_async_resources(r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandResourceSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), bytes, &reason, None).await {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
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
        s: &'a OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let context = s.context();
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_async_resources_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandResourceSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), bytes, &reason, Some(context.clone()))
                        .await
                    {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &context),
                    });
                }
            }
        }
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
        s: &'a OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        let context = s.context();
        loop {
            match self.submit_async_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), 0, &reason, Some(context.clone())).await {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &context),
                    });
                }
            }
        }
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
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Waker;

    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::StopReason;
    use qubit_operation::TimerRegistration;

    use super::wait_retry_tick;

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
        assert!(matches!(retry.as_mut().poll(&mut cx), Poll::Ready(false)));
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
        assert!(matches!(retry.as_mut().poll(&mut cx), Poll::Ready(false)));
        assert_eq!(context.stop_reason(), Some(StopReason::Timeout));
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
    use crate::CommandExecutorApi;
    use crate::CommandResourceRunOptions;
    use crate::CommandRunner;

    fn setup() -> (OperationManager, CommandExecutor, CommandRunner) {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let executor =
            CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).expect("executor should start");
        (manager, executor, CommandRunner::new(Duration::from_secs(5)))
    }

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
                        runner,
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
                Ok::<_, crate::CommandError>((direct, resources))
            })
            .expect("parent operation should complete");
        match result {
            OperationOutcome::Succeeded((direct, resources)) => {
                assert_eq!(direct, b"trait-submit-in");
                assert_eq!(resources, b"trait-resources-in");
            }
            other => panic!("scoped trait submission failed: {other:?}"),
        }
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

        use super::Command;
        use super::CommandExecutorApi;
        use super::CommandResourceRunOptions;
        use super::Duration;
        use super::OperationOutcome;
        use super::setup;

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

        async fn async_submit_and_wait_variants_run_and_settle_commands_inner() {
            let (manager, executor, runner) = setup();
            assert_eq!(
                output_and_settle(
                    executor
                        .submit_async(
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
                    executor
                        .submit_async_resources(
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
                    executor
                        .submit_wait(
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
                    executor
                        .submit_wait_resources(
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
                        scoped_executor
                            .submit_async_in(
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
                        scoped_executor
                            .submit_async_resources_in(
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
                        scoped_executor
                            .submit_wait_in(
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
                        scoped_executor
                            .submit_wait_resources_in(
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
