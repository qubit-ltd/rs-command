/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Coverage support tests.

mod coverage_support_tests;

pub(crate) use coverage_support_tests::command_runner;
pub use coverage_support_tests::{
    CommandError,
    CommandOutput,
    OutputStream,
};
