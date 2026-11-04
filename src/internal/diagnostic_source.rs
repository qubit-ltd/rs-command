// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Controlled summaries for diagnostic error sources.

use std::io;

/// Returns stable I/O metadata without formatting an arbitrary source message.
///
/// # Parameters
///
/// * `source` — the I/O error whose kind and raw operating-system code are
///   summarized.
pub(crate) fn io_source_summary(source: &io::Error) -> String {
    format!("kind={:?}, raw_os_error={:?}", source.kind(), source.raw_os_error())
}

/// Fixed marker used instead of formatting a time error source.
pub(crate) const TIME_SOURCE_SUMMARY: &str = "<time error details omitted>";
