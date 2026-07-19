// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Captured output bytes plus truncation metadata.
#[derive(Debug)]
pub(in crate::command_runner) struct CapturedOutput {
    /// Bytes retained in memory.
    pub(in crate::command_runner) bytes: Vec<u8>,
    /// Whether emitted bytes exceeded the configured retention limit.
    pub(in crate::command_runner) truncated: bool,
}
