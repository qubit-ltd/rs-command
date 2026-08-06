// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private command-runner implementation details.

pub(in crate::command_runner) mod cancel;
pub(in crate::command_runner) mod cancellable_reader;
pub(in crate::command_runner) mod captured_output;
pub(in crate::command_runner) mod command_io;
pub(in crate::command_runner) mod error_mapping;
pub(in crate::command_runner) mod finished_command;
pub(in crate::command_runner) mod io_files;
pub(in crate::command_runner) mod managed_child_process;
pub(in crate::command_runner) mod output_capture_error;
pub(in crate::command_runner) mod output_capture_options;
pub(in crate::command_runner) mod output_collector;
pub(in crate::command_runner) mod output_reader;
pub(in crate::command_runner) mod output_tee;
pub(in crate::command_runner) mod prepared_command;
pub(in crate::command_runner) mod process_launcher;
pub(in crate::command_runner) mod process_setup;
pub(in crate::command_runner) mod process_termination_error;
pub(in crate::command_runner) mod running_command;
pub(in crate::command_runner) mod starting_command;
pub(in crate::command_runner) mod stdin_pipe;
pub(in crate::command_runner) mod stdin_writer;
pub(in crate::command_runner) mod wait_policy;
