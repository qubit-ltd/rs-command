// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared retry decisions for blocking and asynchronous executor submissions.

use qubit_operation::OperationContext;

pub(in crate::execution) use super::wait_result::WaitResult;
use crate::Command;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
use crate::CommandRunner;
use crate::execution::CommandExecutor;
use crate::execution::CommandResourceSubmitFailure;
use crate::execution::CommandSubmitFailure;
use crate::execution::CommandSubmitFailureReason;
#[cfg(feature = "async")]
use crate::execution::command_executor::internal::async_submission_context::AsyncSubmissionContext;
use crate::execution::command_executor::internal::resource_submit_context::ResourceSubmitContext;
use crate::execution::contracts::CapacityFailure;
use crate::execution::executor;
use crate::execution::executor::capture_io_path_base;
use crate::execution::scheduler::BoundedScheduler;
use crate::execution::scheduler::WaitOutcome as SchedulerWaitOutcome;

/// Converts a scheduler wait outcome into the shared executor result.
///
/// # Parameters
/// * `outcome`: Terminal state returned by the scheduler.
fn map_wait_outcome(outcome: SchedulerWaitOutcome) -> WaitResult {
    match outcome {
        SchedulerWaitOutcome::Retry => WaitResult::Retry,
        SchedulerWaitOutcome::Closed => WaitResult::Closed,
        SchedulerWaitOutcome::Stopped => WaitResult::Stopped,
        SchedulerWaitOutcome::DeadlineExpired => WaitResult::DeadlineExpired,
    }
}

impl CommandExecutor {
    /// Implements the scoped blocking wait submission loop.
    ///
    /// # Parameters
    /// * `scope`: Scope whose deadline and stop state govern admission.
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Standard stream options to submit and return on rejection.
    pub(in crate::execution) fn submit_wait_blocking_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::CommandOperation, CommandSubmitFailure> {
        let deadline = scope.context();
        let base = capture_io_path_base();
        let manager = match scope.manager().with_group(&self.group) {
            Ok(manager) => manager,
            Err(error) => {
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::GroupBinding(error),
                });
            }
        };
        loop {
            match self.submit_in_manager(&manager, runner, command, options, Some(deadline.clone()), Some(&base)) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait_outcome(&reason, &self.scheduler, 0, Some(deadline.clone())) {
                        WaitResult::Retry => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        outcome => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the root blocking wait submission loop.
    ///
    /// # Parameters
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Standard stream options to submit and return on rejection.
    pub(in crate::execution) fn submit_wait_blocking_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::CommandOperation, CommandSubmitFailure> {
        let base = capture_io_path_base();
        loop {
            match self.submit_in_manager(&self.manager, runner, command, options, None, Some(&base)) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait_outcome(&reason, &self.scheduler, 0, None) {
                        WaitResult::Retry => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        outcome => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the root blocking resource wait submission loop.
    ///
    /// # Parameters
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Resource options to submit and return on rejection.
    pub(in crate::execution) fn submit_wait_resources_blocking_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOperation, CommandResourceSubmitFailure> {
        let base = capture_io_path_base();
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_resources_manager(
                self,
                &self.manager,
                runner,
                command,
                options,
                bytes,
                ResourceSubmitContext {
                    deadline: None,
                    base: Some(&base),
                },
            ) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait_outcome(&reason, &self.scheduler, bytes, None) {
                        WaitResult::Retry => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        outcome => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the scoped blocking resource wait submission loop.
    ///
    /// # Parameters
    /// * `scope`: Scope whose deadline and stop state govern admission.
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Resource options to submit and return on rejection.
    pub(in crate::execution) fn submit_wait_resources_blocking_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::CommandOperation, CommandResourceSubmitFailure> {
        let deadline = scope.context();
        let base = capture_io_path_base();
        let manager = match scope.manager().with_group(&self.group) {
            Ok(manager) => manager,
            Err(error) => {
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::GroupBinding(error),
                });
            }
        };
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_resources_manager(
                self,
                &manager,
                runner,
                command,
                options,
                bytes,
                ResourceSubmitContext {
                    deadline: Some(deadline.clone()),
                    base: Some(&base),
                },
            ) {
                Ok(operation) => return Ok(operation),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner: failed_runner,
                        command: failed_command,
                        options: failed_options,
                        reason,
                    } = failure;
                    match retry_wait_outcome(&reason, &self.scheduler, bytes, Some(deadline.clone())) {
                        WaitResult::Retry => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        outcome => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Implements the root asynchronous wait submission loop.
    ///
    /// # Parameters
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Standard stream options to submit and return on rejection.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandSubmitFailure> {
        let base = capture_io_path_base();
        loop {
            match self.submit_async_in_manager(
                &self.manager,
                runner,
                command,
                crate::execution::contracts::RequestOptions::Standard(options),
                AsyncSubmissionContext::plain(None).with_io_path_base(base.clone()),
            ) {
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
                        Ok(WaitResult::Retry) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        Ok(outcome) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
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
    ///
    /// # Parameters
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Resource options to submit and return on rejection.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_resources_impl(
        &self,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let base = capture_io_path_base();
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_async_resources_manager(
                self,
                &self.manager,
                runner,
                command,
                options,
                bytes,
                AsyncSubmissionContext::plain(None).with_io_path_base(base.clone()),
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
                        Ok(WaitResult::Retry) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        Ok(outcome) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
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
    ///
    /// # Parameters
    /// * `scope`: Scope whose deadline and stop state govern admission.
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Resource options to submit and return on rejection.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_resources_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandResourceRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let context = scope.context();
        let base = capture_io_path_base();
        let manager = match scope.manager().with_group(&self.group) {
            Ok(manager) => manager,
            Err(error) => {
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::GroupBinding(error),
                });
            }
        };
        loop {
            let bytes = executor::resource_bytes(&options);
            match CommandExecutor::submit_async_resources_manager(
                self,
                &manager,
                runner,
                command,
                options,
                bytes,
                AsyncSubmissionContext::plain(Some(context.clone())).with_io_path_base(base.clone()),
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
                        Ok(WaitResult::Retry) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = *failed_options;
                        }
                        Ok(outcome) => {
                            return Err(CommandResourceSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
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
    ///
    /// # Parameters
    /// * `scope`: Scope whose deadline and stop state govern admission.
    /// * `runner`: Runner configuration to submit and return on rejection.
    /// * `command`: Command to submit and return on rejection.
    /// * `options`: Standard stream options to submit and return on rejection.
    #[cfg(feature = "async")]
    pub(in crate::execution) async fn submit_wait_in_impl(
        &self,
        scope: &qubit_operation::OperationScope,
        mut runner: CommandRunner,
        mut command: Command,
        mut options: CommandRunOptions,
    ) -> Result<crate::execution::AsyncCommandOperation, CommandSubmitFailure> {
        let context = scope.context();
        let base = capture_io_path_base();
        let manager = match scope.manager().with_group(&self.group) {
            Ok(manager) => manager,
            Err(error) => {
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::GroupBinding(error),
                });
            }
        };
        loop {
            match self.submit_async_in_manager(
                &manager,
                runner,
                command,
                crate::execution::contracts::RequestOptions::Standard(options),
                AsyncSubmissionContext::plain(Some(context.clone())).with_io_path_base(base.clone()),
            ) {
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
                        Ok(WaitResult::Retry) => {
                            runner = *failed_runner;
                            command = *failed_command;
                            options = failed_options;
                        }
                        Ok(outcome) => {
                            return Err(CommandSubmitFailure {
                                runner: failed_runner,
                                command: failed_command,
                                options: failed_options,
                                reason: wait_result_reason(reason, outcome),
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
///
/// # Parameters
/// * `reason`: Failure that determines which scheduler condition to await.
/// * `scheduler`: Scheduler that owns admission and waiter state.
/// * `bytes`: Estimated input size used by the capacity predicate.
/// * `context`: Optional scope used to observe stop and deadline state.
pub(in crate::execution) fn retry_wait_outcome(
    reason: &CommandSubmitFailureReason,
    scheduler: &BoundedScheduler,
    bytes: usize,
    context: Option<OperationContext>,
) -> WaitResult {
    match reason {
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull) => scheduler
            .wait_for_capacity_registered_outcome(bytes, context)
            .map(map_wait_outcome)
            .unwrap_or_else(WaitResult::Failed),
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy) => scheduler
            .wait_for_retry_tick_blocking_outcome(context, std::time::Duration::from_millis(10))
            .map(map_wait_outcome)
            .unwrap_or_else(WaitResult::Failed),
        _ => WaitResult::NotRetryable,
    }
}

#[cfg(test)]
pub(in crate::execution) fn retry_wait(
    reason: &CommandSubmitFailureReason,
    scheduler: &BoundedScheduler,
    bytes: usize,
    context: Option<OperationContext>,
) -> Result<bool, crate::execution::CommandSubmitWaitFailure> {
    match retry_wait_outcome(reason, scheduler, bytes, context) {
        WaitResult::Retry => Ok(true),
        WaitResult::Failed(failure) => Err(failure),
        WaitResult::Closed | WaitResult::Stopped | WaitResult::DeadlineExpired | WaitResult::NotRetryable => Ok(false),
    }
}

/// Converts a terminal wait result into the submission failure to return.
///
/// Closure, stop, deadline, and waiter failures take their corresponding
/// submission classifications. Retryable and non-retryable results preserve
/// the original reason.
///
/// # Parameters
/// * `reason`: Original submission failure that initiated the wait.
/// * `result`: Terminal state observed while waiting.
fn wait_result_reason(reason: CommandSubmitFailureReason, result: WaitResult) -> CommandSubmitFailureReason {
    match result {
        WaitResult::Closed => CommandSubmitFailureReason::Capacity(CapacityFailure::Closed),
        WaitResult::Stopped => CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Stopped),
        WaitResult::DeadlineExpired => CommandSubmitFailureReason::Capacity(CapacityFailure::DeadlineExpired),
        WaitResult::Failed(wait) => CommandSubmitFailureReason::Wait(wait),
        WaitResult::Retry | WaitResult::NotRetryable => reason,
    }
}

/// Applies scope stop and deadline precedence after a scoped wait ends.
///
/// # Parameters
/// * `reason`: Failure to return when no stop or deadline takes precedence.
/// * `context`: Scope state checked for deadline and stop signals.
#[cfg(test)]
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

/// Resolves an async wait termination as scope stop, deadline, or closure.
///
/// # Parameters
/// * `reason`: Submission failure that initiated the wait.
/// * `scheduler`: Scheduler whose closure state may have ended the wait.
/// * `context`: Optional scope state checked before reporting scheduler
///   closure.
#[cfg(all(test, feature = "async"))]
pub(in crate::execution) fn async_wait_failure_reason(
    reason: CommandSubmitFailureReason,
    scheduler: &BoundedScheduler,
    context: Option<&OperationContext>,
) -> CommandSubmitFailureReason {
    let reason = if let Some(context) = context {
        deadline_failure(reason, context)
    } else {
        reason
    };
    if scheduler.is_closed()
        && matches!(
            reason,
            CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull)
        )
    {
        CommandSubmitFailureReason::Capacity(CapacityFailure::Closed)
    } else {
        reason
    }
}

/// Reports whether an async submission failure can make progress by waiting.
///
/// # Parameters
/// * `reason`: Submission failure to classify.
#[cfg(feature = "async")]
pub(in crate::execution) fn async_capacity_retry(reason: &CommandSubmitFailureReason) -> bool {
    matches!(
        reason,
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull)
    )
}

/// Reports whether an async submission failure can make progress by waiting.
///
/// # Parameters
/// * `reason`: Submission failure to classify.
#[cfg(feature = "async")]
pub(in crate::execution) fn retryable_async(reason: &CommandSubmitFailureReason) -> bool {
    async_capacity_retry(reason)
        || matches!(
            reason,
            CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy)
        )
}

#[cfg(all(test, feature = "async"))]
mod tests {
    use super::async_wait_failure_reason;
    use crate::execution::CommandExecutionConfig;
    use crate::execution::CommandSubmitFailureReason;
    use crate::execution::contracts::CapacityFailure;
    use crate::execution::contracts::Scheduler;
    use crate::execution::scheduler::BoundedScheduler;

    #[test]
    fn test_async_wait_failure_maps_closed_scheduler_capacity() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig::default()).unwrap();
        Scheduler::close_admission(&scheduler);

        let reason = async_wait_failure_reason(
            CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull),
            &scheduler,
            None,
        );

        assert_eq!(reason, CommandSubmitFailureReason::Capacity(CapacityFailure::Closed));
    }
}
