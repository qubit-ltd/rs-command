// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::sync::Mutex;

use qubit_operation::ResourceRef;

use crate::resource_run_options::FileSinkPolicy;

/// One prepared sink with shared write-safe access and its explicit
/// disposition.
pub(crate) struct PreparedSink {
    /// Stores the file value used by this execution component.
    pub(crate) file: ResourceRef<Mutex<File>>,
    /// Stores the policy value used by this execution component.
    pub(crate) policy: FileSinkPolicy,
}
