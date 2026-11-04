// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Readiness observation used to bound the next budgeted pipe read.
/// No output budget is reserved by this observation.
pub(in crate::command_runner) enum OutputReadiness {
    /// Allows a read capped by the supplied byte capacity, the destination
    /// buffer, and the remaining total output budget. On Unix the capacity
    /// is the 8 KiB read-chunk cap after poll reports readiness, not an exact
    /// available-byte count. On Windows it is the count observed by
    /// PeekNamedPipe at the time of the probe.
    Readable(
        /// Maximum byte capacity supplied by the readiness observation.
        usize,
    ),
    /// Indicates that the Windows pipe probe observed the pipe closed.
    #[cfg(windows)]
    Eof,
    /// Indicates helper cancellation; this does not establish stream EOF.
    Cancelled,
}
