// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Shared retry decisions for blocking and asynchronous executor submissions.

use qubit_operation::OperationContext;

use crate::execution::contracts::CapacityFailure;
use crate::execution::scheduler::BoundedScheduler;
use crate::execution::CommandExecutor;
use crate::execution::CommandExecutorApi;
use crate::execution::CommandResourceSubmitFailure;
use crate::execution::CommandSubmitFailure;
use crate::execution::CommandSubmitFailureReason;
#[cfg(feature = "async")]
use crate::execution::command_executor::internal::async_submission_context::AsyncSubmissionContext;
use crate::execution::executor;
use crate::Command;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
use crate::CommandRunner;

impl CommandExecutor {
    /// Implements the scoped blocking wait submission loop.
    pub(in crate::execution) fn submit_wait_blocking_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::CommandOperation, CommandSubmitFailure> {
        let deadline = scope.context();
        loop {
            match CommandExecutorApi::submit_in(self, scope, runner, command, options) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait(&reason, &self.scheduler, 0, Some(deadline.clone())) {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: deadline_failure(reason, &deadline),
                            });
                        }
                        Err(wait) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the root blocking wait submission loop.
    pub(in crate::execution) fn submit_wait_blocking_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::CommandOperation, CommandSubmitFailure> {
        loop {
            match CommandExecutorApi::submit(self, runner, command, options) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait(&reason, &self.scheduler, 0, None) {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason,
                            });
                        }
                        Err(wait) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the root blocking resource wait submission loop.
    pub(in crate::execution) fn submit_wait_resources_blocking_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOperation, CommandResourceSubmitFailure> {
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_resources_manager(
                self,
                &self.manager,
                runner,
                command,
                options,
                bytes,
                None,
            ) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait(&reason, &self.scheduler, bytes, None) {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason,
                            });
                        }
                        Err(wait) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the scoped blocking resource wait submission loop.
    pub(in crate::execution) fn submit_wait_resources_blocking_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOperation, CommandResourceSubmitFailure> {
        let deadline = scope.context();
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_resources_in_with_bytes(
                self, scope, runner, command, options, bytes,
            ) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait(&reason, &self.scheduler, bytes, Some(deadline.clone())) {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: deadline_failure(reason, &deadline),
                            });
                        }
                        Err(wait) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the root asynchronous wait submission loop.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandSubmitFailure> {
        loop {
            match CommandExecutorApi::submit_async(self, runner, command, options) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    if !retryable_async(&reason) {
                        return Err(CommandSubmitFailure {
                            runner: failed_runner,
                            command: failed_command,
                            options: failed_options,
                            reason,
                        });
                    }
                    match executor::wait_retryable_async(self.scheduler.as_ref().clone(), 0, &reason, None).await {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason,
                            });
                        }
                        Err(wait) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the root asynchronous resource wait submission loop.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_resources_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandResourceSubmitFailure> {
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_async_resources_manager(
                self,
                &self.manager,
                runner,
                command,
                options,
                bytes,
                AsyncSubmissionContext::plain(None),
            ) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    if !retryable_async(&reason) {
                        return Err(CommandResourceSubmitFailure {
                            runner: failed_runner,
                            command: failed_command,
                            options: failed_options,
                            reason,
                        });
                    }
                    match executor::wait_retryable_async(self.scheduler.as_ref().clone(), bytes, &reason, None).await {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason,
                            });
                        }
                        Err(wait) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the scoped asynchronous resource wait submission loop.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_resources_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let context = scope.context();
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_async_resources_in_with_bytes(
                self, scope, runner, command, options, bytes,
            ) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    if !retryable_async(&reason) {
                        return Err(CommandResourceSubmitFailure {
                            runner: failed_runner,
                            command: failed_command,
                            options: failed_options,
                            reason,
                        });
                    }
                    match executor::wait_retryable_async(
                        self.scheduler.as_ref().clone(),
                        bytes,
                        &reason,
                        Some(context.clone()),
                    )
                    .await
                    {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: deadline_failure(reason, &context),
                            });
                        }
                        Err(wait) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the scoped asynchronous standard-option wait submission loop.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandSubmitFailure> {
        let context = scope.context();
        loop {
            match CommandExecutorApi::submit_async_in(self, scope, runner, command, options) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    if !retryable_async(&reason) {
                        return Err(CommandSubmitFailure {
                            runner: failed_runner,
                            command: failed_command,
                            options: failed_options,
                            reason,
                        });
                    }
                    match executor::wait_retryable_async(
                        self.scheduler.as_ref().clone(),
                        0,
                        &reason,
                        Some(context.clone()),
                    )
                    .await
                    {
                        Ok(true) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        Ok(false) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: deadline_failure(reason, &context),
                            });
                        }
                        Err(wait) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: CommandSubmitFailureReason::Wait(wait),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Waits for the scheduler condition represented by a submission failure.
pub(in crate::execution) fn retry_wait(
    reason: &CommandSubmitFailureReason,
    scheduler: &BoundedScheduler,
    bytes: usize,
    context: Option<OperationContext>,
) -> Result<bool, crate::execution::CommandSubmitWaitFailure> {
    match reason {
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull) => {
            scheduler.wait_for_capacity_registered(bytes, context)
        }
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy) => scheduler
            .wait_for_retry_tick_blocking(context, std::time::Duration::from_millis(10)),
        _ => Ok(false),
    }
}

/// Applies scope stop and deadline precedence after a scoped wait ends.
pub(in crate::execution) fn deadline_failure(
    reason: CommandSubmitFailureReason,
    context: &OperationContext,
) -> CommandSubmitFailureReason {
    if context.is_expired() {
        CommandSubmitFailureReason::Capacity(CapacityFailure::DeadlineExpired)
    } else if context.stop_reason().is_some() {
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Stopped)
    } else {
        reason
    }
}

/// Reports whether an async submission failure can make progress by waiting.
#[cfg(feature = "async")]
pub(in crate::execution) fn async_capacity_retry(reason: &CommandSubmitFailureReason) -> bool {
    matches!(
        reason,
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull)
    )
}

/// Reports whether an async submission failure can make progress by waiting.
#[cfg(feature = "async")]
pub(in crate::execution) fn retryable_async(reason: &CommandSubmitFailureReason) -> bool {
    async_capacity_retry(reason)
        || matches!(reason, CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy))
}
