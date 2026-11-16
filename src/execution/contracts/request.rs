// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Submitted command and frozen execution inputs.
use qubit_operation::OperationContext;
use qubit_operation::OperationId;

use super::ExecutionSnapshot;
use super::FactSink;
use super::IoPathSnapshot;
use super::RequestOptions;
use super::Reservation;
use crate::Command;
use crate::CommandRunner;

/// Submitted command and frozen execution inputs.
///
/// `operation_id` is absent before upstream binding and set exactly once before
/// activation; `snapshot` and `io_paths` are captured before admission side
/// effects, while actual file identity checks and file access remain worker
/// responsibilities.
pub struct Request {
    /// Stores the operation id value used by this execution component.
    pub operation_id: Option<OperationId>,
    /// Stores the runner value used by this execution component.
    pub runner: CommandRunner,
    /// Stores the command value used by this execution component.
    pub command: Command,
    /// Stores the options value used by this execution component.
    pub options: RequestOptions,
    /// Stores the snapshot value used by this execution component.
    pub snapshot: ExecutionSnapshot,
    /// Stores the io paths value used by this execution component.
    pub io_paths: IoPathSnapshot,
    /// Inherited operation context used to observe the fixed total deadline in
    /// its clock domain.
    pub deadline: Option<OperationContext>,
}

impl std::fmt::Debug for Request {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `formatter`: Supplies the formatter value used by this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Request")
            .field("operation_id", &self.operation_id)
            .field("runner", &self.runner)
            .field("command", &self.command)
            .field("options", &self.options)
            .field("snapshot", &self.snapshot)
            .field("io_paths", &self.io_paths)
            .field("deadline_configured", &self.deadline.is_some())
            .finish()
    }
}
