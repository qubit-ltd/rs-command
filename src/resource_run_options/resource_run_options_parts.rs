// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal fields transferred from public resource options to preparation.

use std::fs::File;

use super::owned_sink::OwnedSink;
use crate::CommandRunOptions;

/// Crate-private resource settings consumed by owned-file preparation.
#[derive(Debug)]
pub(crate) struct ResourceRunOptionsParts {
    /// Ordinary command settings used for this run.
    pub(crate) options: CommandRunOptions,
    /// Optional owned input file, read from its current cursor.
    pub(crate) stdin: Option<File>,
    /// Optional stdout sink and its explicit content policy.
    pub(crate) stdout: Option<OwnedSink>,
    /// Optional stderr sink and its explicit content policy.
    pub(crate) stderr: Option<OwnedSink>,
}
