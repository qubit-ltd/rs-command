// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Failure data retained by managed file identity validation.

use std::io;

/// Identifies the stream and source of a configured file validation failure.
pub(super) struct IdentityFailure {
    /// Stream whose handle failed validation.
    pub(super) direction: crate::FilePreparationDirection,
    /// Original error produced while inspecting the configured file.
    pub(super) source: io::Error,
}
