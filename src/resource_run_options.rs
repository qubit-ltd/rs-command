// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owned-resource options for managed command execution.

mod command_resource_run_options;
mod file_sink_policy;
mod owned_sink;
mod resource_run_options_parts;

pub use command_resource_run_options::CommandResourceRunOptions;
pub use file_sink_policy::FileSinkPolicy;
