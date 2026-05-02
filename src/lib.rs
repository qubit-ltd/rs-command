/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # Qubit Command
//!
//! Provides command-line process running utilities for Rust.
//!

mod command;
mod command_env;
mod command_error;
mod command_output;
mod command_runner;
mod command_stdin;
#[cfg(coverage)]
#[doc(hidden)]
pub mod coverage_support;
mod output_stream;

pub use command::Command;
pub use command_error::CommandError;
pub use command_output::CommandOutput;
pub use command_runner::{
    CommandRunner,
    DEFAULT_COMMAND_TIMEOUT,
};
pub use output_stream::OutputStream;
