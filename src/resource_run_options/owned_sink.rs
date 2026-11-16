// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal owner for a managed command output sink.

use std::fs::File;

use crate::FileSinkPolicy;

/// File handle paired with the caller-selected content policy.
#[derive(Debug)]
pub(crate) struct OwnedSink {
    /// Output file transferred to the command worker.
    pub(crate) file: File,
    /// Treatment applied to existing contents before the command starts.
    pub(crate) policy: FileSinkPolicy,
}
