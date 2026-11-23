// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Shared retry decisions for blocking and asynchronous executor submissions.

use qubit_operation::OperationContext;

use crate::execution::CommandSubmitFailureReason;
use crate::execution::contracts::CapacityFailure;
use crate::execution::scheduler::BoundedScheduler;

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
