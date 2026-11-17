// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::CommandExecutionConfig;
use super::CommandExecutorCreateError;
use super::adapter;
use super::backend;
use super::executor;
use super::scheduler;

/// Executor configuration bound to an operation manager.
#[derive(Clone)]
pub struct CommandExecutor {
    /// Stores the manager value used by this execution component.
    pub(super) manager: qubit_operation::OperationManager,
    /// Stores the config value used by this execution component.
    pub(super) config: CommandExecutionConfig,
    /// Stores the scheduler value used by this execution component.
    pub(crate) scheduler: std::sync::Arc<scheduler::BoundedScheduler>,
    /// Stores the backend value used by this execution component.
    pub(crate) backend: std::sync::Arc<backend::ProcessBackend>,
    /// Stores the group value used by this execution component.
    pub(crate) group: qubit_operation::OperationGroup,
    /// Stores the recovery value used by this execution component.
    pub(crate) recovery: adapter::RecoverySender,
}
impl CommandExecutor {
    /// Performs the new step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `manager`: Supplies the manager value used by this operation.
    /// * `config`: Supplies the config value used by this operation.
    pub fn new(
        manager: qubit_operation::OperationManager,
        config: CommandExecutionConfig,
    ) -> Result<Self, CommandExecutorCreateError> {
        config.validate()?;
        let (scheduler, backend, group, recovery, manager) = executor::build_executor(manager.clone(), config.clone())?;
        Ok(Self {
            manager,
            config,
            scheduler: std::sync::Arc::new(scheduler),
            backend: std::sync::Arc::new(backend),
            group,
            recovery,
        })
    }
    /// Returns the immutable executor limits.
    pub fn config(&self) -> &CommandExecutionConfig {
        &self.config
    }
    /// Returns the manager view used for future submissions.
    pub fn manager(&self) -> &qubit_operation::OperationManager {
        &self.manager
    }
}
