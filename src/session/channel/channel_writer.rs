// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use super::ByteChannel;

/// RAII owner for the process worker side of a byte channel.
pub(crate) struct ChannelWriter(
    /// Channel whose endpoint lifecycle is managed by this wrapper.
    pub(crate) Arc<ByteChannel>,
);

impl Drop for ChannelWriter {
    /// Signals closure of the worker-owned channel half.
    fn drop(&mut self) {
        self.0.close_writer();
    }
}
