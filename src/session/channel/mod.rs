// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded, runtime-independent byte channels used by managed sessions.

#[cfg(feature = "async")]
mod bridge;
#[cfg(feature = "async")]
mod byte_channel;
#[cfg(feature = "async")]
mod channel_reader;
#[cfg(feature = "async")]
mod channel_state;
#[cfg(feature = "async")]
mod channel_writer;
#[cfg(feature = "async")]
mod output_budget;
#[cfg(feature = "async")]
mod output_budget_state;

#[cfg(feature = "async")]
pub(crate) use bridge::Bridge;
#[cfg(feature = "async")]
pub(crate) use byte_channel::ByteChannel;
#[cfg(feature = "async")]
pub(crate) use channel_reader::ChannelReader;
#[cfg(feature = "async")]
pub(crate) use channel_writer::ChannelWriter;
#[cfg(feature = "async")]
pub(crate) use output_budget::OutputBudget;

/// The channel half-set shared by the process worker and session endpoints.
#[cfg(feature = "async")]
pub(crate) struct SessionChannels {
    /// Blocking process-worker bridge for all session streams.
    pub(crate) bridge: std::sync::Arc<Bridge>,
    /// Caller endpoint for bytes sent to the child.
    pub(crate) stdin: super::endpoints::CommandStdin,
    /// Caller endpoint for bytes received from child stdout.
    pub(crate) stdout: super::endpoints::CommandStdout,
    /// Caller endpoint for bytes received from child stderr.
    pub(crate) stderr: super::endpoints::CommandStderr,
    /// Worker-owned reader for bytes written through stdin; dropping it
    /// indicates that the child stopped reading.
    pub(crate) stdin_reader: ChannelReader,
    /// Worker-owned writer that publishes child stdout; dropping it publishes
    /// end of stream.
    pub(crate) stdout_writer: ChannelWriter,
    /// Worker-owned writer that publishes child stderr.
    pub(crate) stderr_writer: ChannelWriter,
}

#[cfg(feature = "async")]
impl SessionChannels {
    /// Creates session channels with bounded backpressure.
    pub(crate) fn new() -> Self {
        Self::with_policy(crate::session::contracts::SessionStreamPolicy::Backpressure)
    }

    /// Creates session channels using one output policy for both streams.
    ///
    /// # Parameters
    ///
    /// * `policy` - Behavior applied when either output channel fills.
    pub(crate) fn with_policy(policy: crate::session::contracts::SessionStreamPolicy) -> Self {
        let input = std::sync::Arc::new(ByteChannel::new());
        let budget = match policy {
            crate::session::contracts::SessionStreamPolicy::TerminateAfterBytes(limit) => {
                Some(std::sync::Arc::new(OutputBudget::new(limit)))
            }
            _ => None,
        };
        let output = std::sync::Arc::new(ByteChannel::with_policy(policy, budget.clone()));
        let error = std::sync::Arc::new(ByteChannel::with_policy(policy, budget.clone()));
        Self {
            bridge: std::sync::Arc::new(Bridge {
                input: input.clone(),
                output: output.clone(),
                error: error.clone(),
                output_budget: budget,
            }),
            stdin: super::endpoints::CommandStdin::new(input.clone()),
            stdout: super::endpoints::CommandStdout::new(output.clone()),
            stderr: super::endpoints::CommandStderr::new(error.clone()),
            stdin_reader: ChannelReader(input),
            stdout_writer: ChannelWriter(output),
            stderr_writer: ChannelWriter(error),
        }
    }
}

#[cfg(all(test, feature = "async"))]
impl Bridge {
    /// Waits until the stdout writer reaches its bounded capacity in tests.
    pub(crate) fn wait_for_stdout_writer_for_test(&self) {
        self.output.wait_until_blocked();
    }
}
