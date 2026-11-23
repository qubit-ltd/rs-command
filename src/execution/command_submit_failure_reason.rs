// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::CapacityFailure;
use super::SnapshotFailure;

/// Failure while waiting to retry a managed command submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandSubmitWaitFailure {
    /// The executor has reached its configured limit for concurrent waiters.
    WaiterLimit,
}

/// Input rejection reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandSubmitFailureReason {
    /// Describes the capacity case handled by managed command execution.
    Capacity(#[doc = "The scheduler capacity limit that rejected this submission."] CapacityFailure),
    /// Stores the admission value used by this execution component.
    Admission(#[doc = "The operation manager admission failure."] qubit_operation::AdmissionError),
    /// Stores the group binding value used by this execution component.
    GroupBinding(#[doc = "The operation group binding failure."] qubit_operation::GroupBindingError),
    /// Describes the snapshot case handled by managed command execution.
    Snapshot(#[doc = "The failure to capture a stable submission snapshot."] SnapshotFailure),
    /// Describes a failure while waiting to retry this submission.
    Wait(#[doc = "The reason waiting could not continue."] CommandSubmitWaitFailure),
}
