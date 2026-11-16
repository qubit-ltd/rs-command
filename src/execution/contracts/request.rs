//! Immutable command request payloads and activated ownership bundle.
use super::{FactSink, Reservation};
use crate::{Command, CommandResourceRunOptions, CommandRunOptions, CommandRunner};
use qubit_operation::OperationScope;
use std::time::Instant;
/// Submitted command and frozen execution inputs.
#[derive(Debug)]
pub struct Request {
    pub runner: CommandRunner,
    pub command: Command,
    pub options: RequestOptions,
    pub deadline: Option<Instant>,
}
/// Either cloneable collection settings or uniquely owned resource settings.
#[derive(Debug)]
pub enum RequestOptions {
    Standard(CommandRunOptions),
    Resources(CommandResourceRunOptions),
}
/// Activated request exclusively held by a domain worker.
pub struct ActivatedRequest {
    pub scope: OperationScope,
    pub request: Request,
    pub reservation: Reservation,
    pub facts: std::sync::Arc<dyn FactSink>,
}
