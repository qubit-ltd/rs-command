//! Caller-facing stream endpoints. Concrete channel behavior is implemented in T5.
/// Input endpoint returned by `CommandSession::take_stdin`.
#[cfg(feature = "async")]
pub struct CommandStdin;
/// Standard output endpoint returned by `CommandSession::take_stdout`.
#[cfg(feature = "async")]
pub struct CommandStdout;
/// Standard error endpoint returned by `CommandSession::take_stderr`.
#[cfg(feature = "async")]
pub struct CommandStderr;
