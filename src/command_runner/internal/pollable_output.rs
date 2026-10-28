// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
/// Result of waiting for one output pipe without reserving output budget.
pub(in crate::command_runner) enum OutputReadiness {
    Readable(usize),
    #[cfg(windows)]
    Eof,
    Cancelled,
}
