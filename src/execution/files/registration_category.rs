// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Display adapter for a resource registration failure category.

/// Retains the manager's category as the source of an I/O error.
#[derive(Debug)]
pub(crate) struct RegistrationCategory(
    /// Manager classification returned when resource registration fails.
    pub(crate) qubit_operation::ResourceManageErrorKind,
);

impl std::fmt::Display for RegistrationCategory {
    /// Formats the category for inspection in the retained source chain.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Destination receiving the formatted category.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "resource registration failed: {:?}", self.0)
    }
}

impl std::error::Error for RegistrationCategory {}
