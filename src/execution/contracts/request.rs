//! Submitted request snapshots and activated ownership bundle.
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use qubit_operation::OperationContext;
use qubit_operation::OperationId;
use qubit_operation::OperationScope;

use super::FactSink;
use super::Reservation;
use crate::Command;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
use crate::CommandRunner;

/// Submitted command and frozen execution inputs.
///
/// `operation_id` is absent before upstream binding and set exactly once before
/// activation; `snapshot` and `io_paths` are captured before admission side
/// effects, while actual file identity checks and file access remain worker
/// responsibilities.
pub struct Request {
    pub operation_id: Option<OperationId>,
    pub runner: CommandRunner,
    pub command: Command,
    pub options: RequestOptions,
    pub snapshot: ExecutionSnapshot,
    pub io_paths: IoPathSnapshot,
    /// Inherited operation context used to observe the fixed total deadline in
    /// its clock domain.
    pub deadline: Option<OperationContext>,
}

impl std::fmt::Debug for Request {
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

/// Immutable working-directory and environment view captured at submission.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionSnapshot {
    pub working_directory: PathBuf,
    pub environment: Vec<(OsString, Option<OsString>)>,
}

/// Lexical I/O paths captured with the request; file identity is checked later
/// by preparation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IoPathSnapshot {
    pub stdin: Option<PathBuf>,
    pub stdout: Option<PathBuf>,
    pub stderr: Option<PathBuf>,
}

/// Either cloneable collection settings or uniquely owned resource settings.
#[derive(Debug)]
pub enum RequestOptions {
    Standard(CommandRunOptions),
    Resources(CommandResourceRunOptions),
}

/// Activated request exclusively held by a domain worker.
pub struct ActivatedRequest {
    pub operation_id: OperationId,
    pub scope: OperationScope,
    pub request: Request,
    pub reservation: Reservation,
    pub facts: Arc<dyn FactSink>,
    pub session_bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
}
