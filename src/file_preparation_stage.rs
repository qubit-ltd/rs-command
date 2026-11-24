// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Phase of managed file preparation that failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FilePreparationStage {
    /// Configuration or file identity validation. For resource-backed paths,
    /// this includes stdin open/type failures and tee validation failures.
    Validation,
    /// Operation-manager resource reservation.
    ResourceReservation,
    /// Registration of an owned file with the operation scope.
    Registration,
    /// Truncating an output sink failed after validation succeeded.
    Truncation,
    /// Rewinding a truncated output sink.
    Rewind,
}
