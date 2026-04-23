/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Qubit Command
//!
//! Provides command-line process running utilities for Rust.
//!
//! # Author
//!
//! Haixing Hu

mod command;
mod command_error;
mod command_output;
mod command_runner;
mod output_stream;

pub use command::Command;
pub use command_error::CommandError;
pub use command_output::CommandOutput;
#[cfg(coverage)]
#[doc(hidden)]
pub use command_runner::coverage_support;
pub use command_runner::{
    CommandRunner,
    DEFAULT_COMMAND_TIMEOUT,
};
pub use output_stream::OutputStream;
