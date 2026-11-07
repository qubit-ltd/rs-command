// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared support types for command-runner integration tests.

#![allow(dead_code)]

#[cfg(not(windows))]
pub(crate) mod captured_logger;
pub(crate) mod documentation_dependency_baseline;
pub(crate) mod documentation_examples;
#[cfg(target_os = "linux")]
pub(crate) mod escaped_process_guard;
#[cfg(unix)]
pub(crate) mod managed_test_process;
#[allow(dead_code)]
pub(crate) mod output_command;
#[cfg(not(windows))]
pub(crate) mod switching_timer;
pub(crate) mod temp_dir;
pub(crate) mod total_output_process;
