// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! # Qubit Command
//!
//! Provides command-line process running utilities for Rust.

mod command;
mod command_argument;
mod command_cancellation;
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

pub use command::Command;
pub use command_cancellation::CommandCancellation;
pub use command_cleanup_failure::CommandCleanupFailure;
pub use command_error::CommandError;
pub use command_error_kind::CommandErrorKind;
pub use command_error_reason::CommandErrorReason;
pub use command_output::CommandOutput;
pub use command_run_options::CommandRunOptions;
#[cfg(coverage)]
#[doc(hidden)]
pub use command_runner::__coverage_internal;
pub use command_runner::CommandRunner;
pub use command_runner::DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM;
pub use output_stream::OutputStream;
