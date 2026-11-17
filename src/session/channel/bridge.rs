// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Synchronous process-worker access to managed session byte channels.

use std::sync::Arc;

use super::byte_channel::ByteChannel;
use super::output_budget::OutputBudget;
use crate::session::contracts::SessionBridge;

/// Connects blocking process workers to caller-facing session streams.
pub(crate) struct Bridge {
    /// Child stdin channel.
    pub(super) input: Arc<ByteChannel>,
    /// Child stdout channel.
    pub(super) output: Arc<ByteChannel>,
    /// Child stderr channel.
    pub(super) error: Arc<ByteChannel>,
    /// Shared aggregate budget for stdout and stderr, when configured.
    pub(super) output_budget: Option<Arc<OutputBudget>>,
}

#[cfg(feature = "async")]
impl SessionBridge for Bridge {
    /// Reads the next bytes supplied through the session stdin endpoint.
    ///
    /// # Parameters
    ///
    /// * `buffer` - Destination for the next available input bytes.
    fn read_stdin(&self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.input.blocking_read(buffer)
    }
    /// Publishes child stdout bytes to the session channel.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Standard output bytes emitted by the child.
    fn write_stdout(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.output.blocking_write(bytes)
    }
    /// Publishes child stderr bytes to the session channel.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Standard error bytes emitted by the child.
    fn write_stderr(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.error.blocking_write(bytes)
    }
    /// Closes stdin so the child observes end of input.
    fn close_stdin(&self) {
        self.input.close_writer();
    }
    /// Stops accepting output after shutdown while preserving buffered bytes.
    fn request_output_stop(&self) {
        self.output.stop_output();
        self.error.stop_output();
    }
    /// Reports whether stdout bytes were discarded.
    fn stdout_incomplete(&self) -> bool {
        self.output.incomplete()
    }
    /// Reports whether stderr bytes were discarded.
    fn stderr_incomplete(&self) -> bool {
        self.error.incomplete()
    }
    /// Reports whether the child observed a closed stdin consumer.
    fn stdin_broken_pipe(&self) -> bool {
        self.input.broken_pipe()
    }
    /// Records a child-side broken pipe on stdin.
    fn record_stdin_broken_pipe(&self) {
        self.input.record_broken_pipe();
    }
    /// Reports whether combined output reached its termination limit.
    fn output_budget_exceeded(&self) -> bool {
        self.output_budget
            .as_ref()
            .is_some_and(|budget| budget.exceeded())
    }
    /// Returns the configured combined output limit.
    fn output_budget_limit(&self) -> Option<u64> {
        self.output_budget.as_ref().map(|budget| budget.limit())
    }
    /// Returns bytes counted against the combined output limit.
    fn output_bytes_observed(&self) -> u64 {
        self.output_budget
            .as_ref()
            .map_or(0, |budget| budget.observed())
    }
}
