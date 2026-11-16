//! Opaque caller-facing stream endpoint types; channel behavior is implemented in T5.
/// Input endpoint returned by `CommandSession::take_stdin`.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub struct CommandStdin {
    pub(crate) state: std::sync::Arc<dyn crate::session::contracts::StdinEndpointState>,
}
/// Standard output endpoint returned by `CommandSession::take_stdout`.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub struct CommandStdout {
    pub(crate) state: std::sync::Arc<dyn crate::session::contracts::OutputEndpointState>,
}
/// Standard error endpoint returned by `CommandSession::take_stderr`.
#[cfg(feature = "async")]
#[allow(dead_code)]
pub struct CommandStderr {
    pub(crate) state: std::sync::Arc<dyn crate::session::contracts::OutputEndpointState>,
}
