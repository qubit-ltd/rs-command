// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Opaque caller-facing stream endpoint types; channel behavior is implemented
//! in T5.
#[cfg(feature = "async")]
mod command_stderr;
#[cfg(feature = "async")]
mod command_stdin;
#[cfg(feature = "async")]
mod command_stdout;
#[cfg(feature = "async")]
pub use command_stderr::CommandStderr;
#[cfg(feature = "async")]
pub use command_stdin::CommandStdin;
#[cfg(feature = "async")]
pub use command_stdout::CommandStdout;
