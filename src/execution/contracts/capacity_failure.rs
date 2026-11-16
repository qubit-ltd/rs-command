// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Reason a reservation could not be obtained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityFailure {
    /// Describes the active full case handled by managed command execution.
    ActiveFull,
    /// Describes the queue full case handled by managed command execution.
    QueueFull,
    /// Describes the input bytes exceeded case handled by managed command
    /// execution.
    InputBytesExceeded,
    /// Describes the deadline expired case handled by managed command
    /// execution.
    DeadlineExpired,
    /// Describes the foreign reservation case handled by managed command
    /// execution.
    ForeignReservation,
    /// Describes the closed case handled by managed command execution.
    Closed,
}
