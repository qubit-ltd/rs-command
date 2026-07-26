// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared support types for command-runner integration tests.

#[cfg(not(windows))]
mod captured_logger;
mod switching_timer;
mod temp_dir;

#[cfg(not(windows))]
pub(crate) use captured_logger::{
    captured_log_records_containing,
    initialize_captured_logger,
};
pub(crate) use switching_timer::SwitchingTimer;
pub(crate) use temp_dir::LocalTempDir;
