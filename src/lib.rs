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
//! Provides command-line process running utilities for Qubit Rust projects.
//!
//! # Author
//!
//! Haixing Hu

mod command_error;
mod command_output;
mod command_runner;
mod command_spec;

pub use command_error::{
    CommandError,
    OutputStream,
};
pub use command_output::CommandOutput;
#[cfg(coverage)]
#[doc(hidden)]
pub use command_runner::coverage_support;
pub use command_runner::{
    CommandRunner,
    DEFAULT_COMMAND_TIMEOUT,
};
pub use command_spec::CommandSpec;
