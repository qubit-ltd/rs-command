/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
/// Captured output bytes plus truncation metadata.
#[derive(Debug)]
pub(crate) struct CapturedOutput {
    /// Bytes retained in memory.
    pub(crate) bytes: Vec<u8>,
    /// Whether emitted bytes exceeded the configured retention limit.
    pub(crate) truncated: bool,
}
