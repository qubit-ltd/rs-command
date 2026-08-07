// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Captured output bytes plus truncation and completion metadata.
#[derive(Debug, Clone)]
pub(in crate::command_runner) struct CapturedOutput {
    /// Bytes retained in memory.
    pub(in crate::command_runner) bytes: Vec<u8>,
    /// Whether emitted bytes exceeded the configured retention limit.
    pub(in crate::command_runner) truncated: bool,
    /// Whether the stream reached EOF rather than being cancelled.
    pub(in crate::command_runner) complete: bool,
}

impl Default for CapturedOutput {
    /// Creates empty, complete output metadata.
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            truncated: false,
            complete: true,
        }
    }
}
