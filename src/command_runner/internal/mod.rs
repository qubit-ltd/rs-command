// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private command-runner implementation details.

pub(crate) mod captured_output;
pub(crate) mod command_io;
pub(crate) mod error_mapping;
pub(crate) mod finished_command;
pub(crate) mod io_files;
pub(crate) mod managed_child_process;
pub(crate) mod output_capture_error;
pub(crate) mod output_capture_options;
pub(crate) mod output_collector;
pub(crate) mod output_reader;
pub(crate) mod output_tee;
pub(crate) mod prepared_command;
pub(crate) mod process_launcher;
pub(crate) mod process_setup;
pub(crate) mod running_command;
pub(crate) mod starting_command;
pub(crate) mod stdin_pipe;
pub(crate) mod stdin_writer;
pub(crate) mod wait_policy;
