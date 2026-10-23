// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsStr;
use std::ffi::OsString;

use qubit_redact::Sensitivity;

/// One raw command argument and its diagnostic sensitivity.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CommandArgument {
    /// Raw value passed to the child process.
    value: OsString,
    /// Optional sensitivity applied only to diagnostic rendering.
    sensitivity: Option<Sensitivity>,
}

impl CommandArgument {
    /// Creates a visible argument.
    ///
    /// # Parameters
    ///
    /// * `value` - Raw argument value passed to the child process.
    ///
    /// # Returns
    ///
    /// An argument that is not redacted by its configured sensitivity.
    #[must_use]
    #[inline]
    pub(crate) const fn visible(value: OsString) -> Self {
        Self {
            value,
            sensitivity: None,
        }
    }

    /// Creates a fully redacted argument.
    ///
    /// # Parameters
    ///
    /// * `value` - Raw argument value passed to the child process.
    ///
    /// # Returns
    ///
    /// An argument marked with secret sensitivity for diagnostic rendering.
    #[must_use]
    #[inline]
    pub(crate) const fn sensitive(value: OsString) -> Self {
        Self {
            value,
            sensitivity: Some(Sensitivity::Secret),
        }
    }

    /// Returns the raw argument value passed to the child process.
    ///
    /// # Returns
    ///
    /// A borrowed operating-system string that preserves non-UTF-8 values.
    #[must_use]
    #[inline]
    pub(crate) fn value(&self) -> &OsStr {
        self.value.as_os_str()
    }

    /// Returns the configured diagnostic sensitivity.
    ///
    /// # Returns
    ///
    /// The sensitivity applied when rendering diagnostics, if one was set.
    #[inline]
    pub(crate) const fn sensitivity(&self) -> Option<Sensitivity> {
        self.sensitivity
    }
}
