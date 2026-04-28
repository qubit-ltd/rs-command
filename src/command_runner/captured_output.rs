/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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
