//! Managed command session and stream endpoint contracts.
pub mod channel;
pub mod contracts;
#[cfg(feature = "async")]
pub use endpoints::{CommandStderr, CommandStdin, CommandStdout};
pub mod endpoints;
/// Streaming session handle; dropping it requests command stop.
#[derive(Debug)]
pub struct CommandSession {
    _private: (),
}

/// Frozen session endpoint accessors implemented by the session integration.
pub trait CommandSessionApi {
    /// Takes the session input endpoint, when available.
    #[cfg(feature = "async")]
    fn take_stdin(&mut self) -> Option<CommandStdin>;
    /// Takes the session stdout endpoint, when available.
    #[cfg(feature = "async")]
    fn take_stdout(&mut self) -> Option<CommandStdout>;
    /// Takes the session stderr endpoint, when available.
    #[cfg(feature = "async")]
    fn take_stderr(&mut self) -> Option<CommandStderr>;
}
