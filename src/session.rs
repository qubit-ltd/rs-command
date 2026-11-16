// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Managed command session and stream endpoint contracts.
pub mod channel;
pub mod contracts;
#[cfg(feature = "async")]
pub use endpoints::CommandStderr;
#[cfg(feature = "async")]
pub use endpoints::CommandStdin;
#[cfg(feature = "async")]
pub use endpoints::CommandStdout;
pub mod endpoints;

/// Streaming session owning its operation handle and any untaken endpoints.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub struct CommandSession {
    /// Managed operation whose outcome and settlement belong to this session.
    pub(crate) operation: Option<crate::CommandOperation>,
    /// Input endpoint retained until taken or the session is dropped.
    pub(crate) stdin: Option<CommandStdin>,
    /// Standard-output endpoint retained until taken or the session is dropped.
    pub(crate) stdout: Option<CommandStdout>,
    /// Standard-error endpoint retained until taken or the session is dropped.
    pub(crate) stderr: Option<CommandStderr>,
}

/// Frozen session observation and endpoint accessors implemented by T6.
#[cfg(feature = "async")]
pub trait CommandSessionApi {
    /// Borrows the operation handle for lifecycle observation.
    fn operation(&self) -> Option<&crate::CommandOperation>;
    /// Takes the operation handle, transferring its unique outcome receiver.
    fn take_operation(&mut self) -> Option<crate::CommandOperation>;
    /// Takes the session input endpoint, when available.
    fn take_stdin(&mut self) -> Option<CommandStdin>;
    /// Takes the session stdout endpoint, when available.
    fn take_stdout(&mut self) -> Option<CommandStdout>;
    /// Takes the session stderr endpoint, when available.
    fn take_stderr(&mut self) -> Option<CommandStderr>;
}
