// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared retry decisions for blocking and asynchronous executor submissions.

use qubit_operation::OperationContext;

use crate::execution::CommandSubmitFailureReason;
use crate::execution::contracts::CapacityFailure;
use crate::execution::scheduler::BoundedScheduler;

/// Waits for the scheduler condition represented by a submission failure.
///
/// # Parameters
/// * `reason`: Failure that determines which scheduler condition to await.
/// * `scheduler`: Scheduler that owns admission and waiter state.
/// * `bytes`: Estimated input size used by the capacity predicate.
/// * `context`: Optional scope used to observe stop and deadline state.
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
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy) => {
            scheduler.wait_for_retry_tick_blocking(context, std::time::Duration::from_millis(10))
        }
        _ => Ok(false),
    }
}

/// Applies scope stop and deadline precedence after a scoped wait ends.
///
/// # Parameters
/// * `reason`: Failure to return when no stop or deadline takes precedence.
/// * `context`: Scope state checked for deadline and stop signals.
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
