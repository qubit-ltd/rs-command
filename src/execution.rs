// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Managed command execution contracts and bounded configuration.
#[path = "execution/adapter/mod.rs"]
pub mod adapter;
#[path = "execution/async_command_operation.rs"]
#[cfg(feature = "async")]
mod async_command_operation;
#[path = "execution/backend/mod.rs"]
pub mod backend;
#[path = "execution/command_execution_config.rs"]
mod command_execution_config;
#[path = "execution/command_executor.rs"]
mod command_executor;
#[path = "execution/command_executor_api.rs"]
mod command_executor_api;
#[path = "execution/command_executor_create_error.rs"]
mod command_executor_create_error;
#[path = "execution/command_operation.rs"]
mod command_operation;
#[path = "execution/command_resource_submit_failure.rs"]
mod command_resource_submit_failure;
#[path = "execution/command_submit_failure.rs"]
mod command_submit_failure;
#[path = "execution/command_submit_failure_reason.rs"]
mod command_submit_failure_reason;
#[path = "execution/contracts/mod.rs"]
pub mod contracts;
#[path = "execution/executor.rs"]
pub(super) mod executor;
#[path = "execution/files/mod.rs"]
pub mod files;
#[path = "execution/internal/mod.rs"]
mod internal;
#[path = "execution/scheduler/mod.rs"]
pub mod scheduler;
#[path = "execution/snapshot_failure.rs"]
mod snapshot_failure;

#[cfg(feature = "async")]
pub use async_command_operation::AsyncCommandOperation;
pub use command_execution_config::CommandExecutionConfig;
pub use command_executor::CommandExecutor;
pub use command_executor_api::CommandExecutorApi;
pub use command_executor_create_error::CommandExecutorCreateError;
pub use command_operation::CommandOperation;
pub use command_resource_submit_failure::CommandResourceSubmitFailure;
pub use command_submit_failure::CommandSubmitFailure;
pub use command_submit_failure_reason::CommandSubmitFailureReason;
pub use contracts::ActivatedRequest;
pub use contracts::Backend;
pub use contracts::CapacityFailure;
pub use contracts::EnqueueFailure;
pub use contracts::FactSink;
pub use contracts::FilePreparationFailure;
pub use contracts::PreparedFiles;
pub use contracts::RecoveryOwner;
pub use contracts::RecoveryPublishError;
pub use contracts::RecoveryPublishFailure;
pub use contracts::RecoveryRetryError;
pub use contracts::Request;
pub use contracts::Reservation;
pub use contracts::ResourceInput;
pub use contracts::ScheduledJob;
pub use contracts::Scheduler;
pub use contracts::StopPort;
pub use snapshot_failure::SnapshotFailure;
