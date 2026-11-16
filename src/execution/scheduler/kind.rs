// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Scheduler-local accounting identity carried by a reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    /// Describes the active case handled by managed command execution.
    Active,
    /// Describes the queued case handled by managed command execution.
    Queued,
}
