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
use super::IoPathSnapshot;
use super::RequestOptions;
use crate::Command;
use crate::CommandRunner;

/// Submitted command and frozen execution inputs.
///
/// `operation_id` is absent before upstream binding and set exactly once before
/// activation; `snapshot` and `io_paths` are captured before admission side
/// effects. The snapshot directory is the parent-directory base for managed
/// path I/O, not the child working directory. Actual file identity checks and
/// file access remain worker responsibilities. The environment compatibility
/// field in the snapshot is empty in production requests; process environment
/// inheritance and command overrides are applied by the child process setup.
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::Request;
    use crate::Command;
    use crate::CommandRunOptions;
    use crate::CommandRunner;
    use crate::execution::contracts::ExecutionSnapshot;
    use crate::execution::contracts::IoPathSnapshot;
    use crate::execution::contracts::RequestOptions;

    #[test]
    fn test_request_debug_summarizes_deadline_and_redacts_nested_command_settings() {
        let request = Request {
            operation_id: None,
            runner: CommandRunner::without_timeout(),
            command: Command::new("worker").sensitive_arg("request-command-secret"),
            options: RequestOptions::Standard(
                CommandRunOptions::new().tee_stdout_to_file("private/request-output-secret.log"),
            ),
            snapshot: ExecutionSnapshot {
                working_directory: PathBuf::from("private/request-working-directory-secret"),
                environment: vec![(
                    OsString::from("PRIVATE_ENVIRONMENT_KEY_SECRET"),
                    Some(OsString::from("private-environment-value-secret")),
                )],
            },
            io_paths: IoPathSnapshot {
                stdin: Some(PathBuf::from("private/request-input-secret")),
                stdout: Some(PathBuf::from("private/request-output-secret.log")),
                stderr: Some(PathBuf::from("private/request-error-secret.log")),
            },
            deadline: None,
        };

        let debug = format!("{request:?}");

        assert!(debug.contains("deadline_configured: false"));
        assert!(debug.contains("environment_entries: 1"));
        assert!(debug.contains("stdin: true"));
        assert!(debug.contains("stdout: true"));
        assert!(!debug.contains("request-command-secret"));
        assert!(!debug.contains("PRIVATE_ENVIRONMENT_KEY_SECRET"));
        assert!(!debug.contains("private-environment-value-secret"));
        assert!(!debug.contains("private/request-working-directory-secret"));
        assert!(!debug.contains("private/request-input-secret"));
        assert!(!debug.contains("private/request-output-secret.log"));
        assert!(!debug.contains("private/request-error-secret.log"));
    }
}
