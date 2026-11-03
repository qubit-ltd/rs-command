// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # Qubit Command
//!
//! Run external programs with explicit policies for process lifetime, output,
//! cancellation, and diagnostics.
//!
//! ```
//! use qubit_command::{Command, CommandRunner};
//! use std::time::Duration;
//!
//! let output = CommandRunner::new(Duration::from_secs(10))
//!     .run(Command::new("rustc").arg("--version"))?;
//! assert!(!output.stdout().is_empty());
//! # Ok::<(), qubit_command::CommandError>(())
//! ```
//!
//! [`Command`] describes a program and its arguments. [`CommandRunner`] runs
//! it and returns [`CommandOutput`] or a structured [`CommandError`].
//! [`CommandRunOptions`] adds per-run settings, while [`CommandCancellation`]
//! allows a caller to request cancellation.
//!
//! By default, the runner retains at most 1 MiB per output stream in memory
//! and treats successful output truncation as an error. These retention limits
//! are separate from the optional combined stdout/stderr budget configured by
//! [`CommandRunner::max_total_output_bytes`]. If that budget is exceeded,
//! [`CommandError::output`] is `None`; inspect partial bytes through
//! [`CommandError::captured_output`].
//!
//! A configured timeout starts after the child is spawned. It does not bound
//! preparation, spawn, or every operating-system operation. Timeout and
//! cancellation errors can contain partial output; check
//! [`CommandOutput::stdout_complete`] and [`CommandOutput::stderr_complete`]
//! before treating retained bytes as complete streams.
//!
//! See the [English user guide](https://github.com/qubit-ltd/rs-command/blob/main/doc/user_guide.md)
//! and [中文用户指南](https://github.com/qubit-ltd/rs-command/blob/main/doc/user_guide.zh_CN.md)
//! for the full usage documentation.

mod command;
mod command_argument;
mod command_cancellation;
mod command_captured_output;
mod command_cleanup_failure;
mod command_env;
mod command_error;
mod command_error_kind;
mod command_error_reason;
mod command_output;
mod command_run_options;
mod command_run_options_parts;
mod command_runner;
mod command_stdin;
mod internal;
mod output_stream;

#[cfg(test)]
mod tests;

pub use command::Command;
pub use command_cancellation::CommandCancellation;
pub use command_captured_output::CommandCapturedOutput;
pub use command_cleanup_failure::CommandCleanupFailure;
pub use command_error::CommandError;
pub use command_error_kind::CommandErrorKind;
pub use command_error_reason::CommandErrorReason;
pub use command_output::CommandOutput;
pub use command_run_options::CommandRunOptions;
pub use command_runner::CommandRunner;
pub use command_runner::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM;
pub use output_stream::OutputStream;
