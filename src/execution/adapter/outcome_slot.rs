// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Outcome storage is shared by the typed handle and lifecycle observer.
/// Stores the outcome value used by this execution component.
///
/// # Type Parameters
/// * `T`: Outcome value returned by the operation.
/// * `E`: Error value returned by the operation.
pub(crate) type Outcome<T, E> = qubit_operation::OperationOutcome<T, E>;
