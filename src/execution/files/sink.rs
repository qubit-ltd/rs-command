// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::io::Write;

use crate::execution::contracts::PreparedSink;

/// Writes tee bytes while serializing helpers that share an owned sink.
///
/// # Parameters
pub(crate) fn write_sink(sink: &PreparedSink, bytes: &[u8]) -> io::Result<()> {
    sink.file
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?
        .write_all(bytes)
}

/// Flushes a shared sink after its final helper write.
///
/// # Parameters
pub(crate) fn flush_sink(sink: &PreparedSink) -> io::Result<()> {
    sink.file
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?
        .flush()
}
