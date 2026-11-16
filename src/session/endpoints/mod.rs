//! Opaque caller-facing stream endpoint types; channel behavior is implemented in T5.
#[cfg(feature = "async")]
mod stderr;
#[cfg(feature = "async")]
mod stdin;
#[cfg(feature = "async")]
mod stdout;
#[cfg(feature = "async")]
pub use stderr::CommandStderr;
#[cfg(feature = "async")]
pub use stdin::CommandStdin;
#[cfg(feature = "async")]
pub use stdout::CommandStdout;
