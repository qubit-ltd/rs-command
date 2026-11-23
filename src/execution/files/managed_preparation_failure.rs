// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;

use crate::execution::contracts::FilePreparationFailure;

/// Typed preparation failure with the public resource recovery carrier.
pub(crate) struct ManagedPreparationFailure {
    /// Stage at which preparation failed.
    pub(crate) phase: crate::FilePreparationStage,
    /// Affected file direction, when the failure belongs to a stream.
    pub(crate) direction: Option<crate::FilePreparationDirection>,
    /// Original error or manager failure summary.
    pub(crate) source: io::Error,
    /// Public recovery value retaining raw and registered owners.
    pub(crate) failure: FilePreparationFailure,
}
