// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Mutable state protected by the managed byte channel's lock.

use std::collections::VecDeque;
use std::task::Waker;

use super::output_budget::OutputBudget;
use crate::session::contracts::SessionStreamPolicy;

/// Queue contents, endpoint lifecycle flags, and pending task wakers.
pub(super) struct ChannelState {
    /// Buffered bytes awaiting delivery to the consumer.
    pub(super) bytes: VecDeque<u8>,
    /// Whether the producer has closed its side of the channel.
    pub(super) writer_closed: bool,
    /// Whether new output should be accepted or discarded.
    pub(super) stop_writes: bool,
    /// Whether the consumer endpoint has been dropped.
    pub(super) reader_gone: bool,
    /// Whether any bytes were discarded before delivery.
    pub(super) incomplete: bool,
    /// Whether the producer observed a broken pipe.
    pub(super) broken_pipe: bool,
    /// Task waiting for bytes to become readable.
    pub(super) read_waker: Option<Waker>,
    /// Task waiting for queue capacity.
    pub(super) write_waker: Option<Waker>,
    /// Output behavior applied when the queue fills.
    pub(super) policy: SessionStreamPolicy,
    /// Optional shared stdout and stderr byte budget.
    pub(super) budget: Option<std::sync::Arc<OutputBudget>>,
}
