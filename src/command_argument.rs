// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::{
    OsStr,
    OsString,
};

use qubit_sanitize::SensitivityLevel;

/// One raw command argument and its diagnostic sensitivity.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CommandArgument {
    /// Raw value passed to the child process.
    value: OsString,
    /// Optional sensitivity applied only to diagnostic rendering.
    sensitivity: Option<SensitivityLevel>,
}

impl CommandArgument {
    /// Creates an argument whose value may be rendered in diagnostics.
    ///
    /// # Parameters
    ///
    /// * `value` - Raw argument value passed to the child process.
    ///
    /// # Returns
    ///
    /// A diagnostically visible command argument.
    #[inline(always)]
    pub(crate) const fn visible(value: OsString) -> Self {
        Self {
            value,
            sensitivity: None,
        }
    }

    /// Creates an argument whose diagnostic value is fully redacted.
    ///
    /// # Parameters
    ///
    /// * `value` - Raw argument value passed to the child process.
    ///
    /// # Returns
    ///
    /// A command argument marked with secret sensitivity.
    #[inline(always)]
    pub(crate) const fn sensitive(value: OsString) -> Self {
        Self {
            value,
            sensitivity: Some(SensitivityLevel::Secret),
        }
    }

    /// Returns the raw argument value.
    ///
    /// # Returns
    ///
    /// Argument value passed to the child process.
    #[inline(always)]
    pub(crate) fn value(&self) -> &OsStr {
        self.value.as_os_str()
    }

    /// Returns the diagnostic sensitivity.
    ///
    /// # Returns
    ///
    /// Explicit sensitivity for diagnostic rendering, if configured.
    #[inline(always)]
    pub(crate) const fn sensitivity(&self) -> Option<SensitivityLevel> {
        self.sensitivity
    }
}
