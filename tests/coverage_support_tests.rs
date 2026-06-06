// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression tests for defensive command-runner behavior.

mod coverage_support;

pub(crate) use coverage_support::command_runner;
pub use coverage_support::{
    CommandError,
    CommandOutput,
    OutputStream,
};
