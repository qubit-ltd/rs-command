// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Policies for preparing owned output files.

/// Explicit treatment of an owned output sink during preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileSinkPolicy {
    /// Preserve existing contents and the file's current cursor; never seek
    /// implicitly.
    Preserve,
    /// Truncate to zero and position at the beginning during worker
    /// preparation.
    Truncate,
}
