//! Bounded, runtime independent byte channels used by managed sessions.
#[cfg(feature = "async")]
mod reader;
#[cfg(feature = "async")]
mod state;
#[cfg(feature = "async")]
mod writer;

#[cfg(feature = "async")]
pub(crate) use reader::ChannelReader;
#[cfg(feature = "async")]
pub(crate) use state::ByteChannel;
#[cfg(feature = "async")]
pub(crate) use state::OutputBudget;
#[cfg(feature = "async")]
pub(crate) use writer::ChannelWriter;

/// The channel half-set shared by the process worker and session endpoints.
#[cfg(feature = "async")]
pub(crate) struct SessionChannels {
    pub(crate) bridge: std::sync::Arc<Bridge>,
    pub(crate) stdin: super::endpoints::CommandStdin,
    pub(crate) stdout: super::endpoints::CommandStdout,
    pub(crate) stderr: super::endpoints::CommandStderr,
    /// Owned by the process worker; dropping it means the child stopped
    /// reading.
    pub(crate) stdin_reader: ChannelReader,
    /// Owned by the process worker; dropping either writer publishes output
    /// EOF.
    pub(crate) stdout_writer: ChannelWriter,
    pub(crate) stderr_writer: ChannelWriter,
}

#[cfg(feature = "async")]
pub(crate) struct Bridge {
    input: std::sync::Arc<ByteChannel>,
    output: std::sync::Arc<ByteChannel>,
    error: std::sync::Arc<ByteChannel>,
    output_budget: Option<std::sync::Arc<OutputBudget>>,
}

#[cfg(feature = "async")]
impl SessionChannels {
    pub(crate) fn new() -> Self {
        Self::with_policy(crate::session::contracts::SessionStreamPolicy::Backpressure)
    }

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

#[cfg(feature = "async")]
impl super::contracts::SessionBridge for Bridge {
    fn read_stdin(&self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.input.blocking_read(buffer)
    }
    fn write_stdout(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.output.blocking_write(bytes)
    }
    fn write_stderr(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.error.blocking_write(bytes)
    }
    fn close_stdin(&self) {
        self.input.close_writer();
    }
    fn request_output_stop(&self) {
        self.output.stop_output();
        self.error.stop_output();
    }
    fn stdout_incomplete(&self) -> bool {
        self.output.incomplete()
    }
    fn stderr_incomplete(&self) -> bool {
        self.error.incomplete()
    }
    fn stdin_broken_pipe(&self) -> bool {
        self.input.broken_pipe()
    }
    fn record_stdin_broken_pipe(&self) {
        self.input.record_broken_pipe();
    }
    fn output_budget_exceeded(&self) -> bool {
        self.output_budget.as_ref().is_some_and(|budget| budget.exceeded())
    }
    fn output_budget_limit(&self) -> Option<u64> {
        self.output_budget.as_ref().map(|budget| budget.limit())
    }
    fn output_bytes_observed(&self) -> u64 {
        self.output_budget.as_ref().map_or(0, |budget| budget.observed())
    }
}

#[cfg(all(test, feature = "async"))]
impl Bridge {
    pub(crate) fn wait_for_stdout_writer_for_test(&self) {
        self.output.wait_until_blocked();
    }
}
