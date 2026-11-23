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
use super::internal::ExecutorLifetime;
use super::scheduler;

#[cfg(feature = "async")]
// Owns the asynchronous retry timer used by executor admission.
#[path = "command_executor/retry_timer/mod.rs"]
mod retry_timer;
#[cfg(feature = "async")]
pub(super) use retry_timer::RetryDelay;

/// Manages local command admission and exposes operation handles tied to an
/// operation manager.
///
/// Use [`CommandExecutionConfig`] to set the executor-local limits. The
/// [`submit`](Self::submit) and [`submit_resources`](Self::submit_resources)
/// methods attempt admission immediately;
/// [`CommandExecutorApi::submit_wait_blocking`](crate::CommandExecutorApi::submit_wait_blocking)
/// waits for capacity; with the `async` feature, `submit_wait` provides the
/// corresponding asynchronous wait. The `*_in` variants use an
/// [`qubit_operation::OperationScope`] group and context. The `submit_async*`
/// methods return an asynchronous operation view but do not themselves wait for
/// capacity.
///
/// A [`crate::CommandOperation`] outcome is one-shot; settlement and any
/// recovery are separate resource lifecycle observations. Dropping the last
/// executor clone closes admission and lets accepted work drain in the
/// background; it does not prove that settlement or recovery has completed.
/// See the [managed execution guide](https://github.com/qubit-ltd/rs-command/blob/main/doc/user_guide.md)
/// and the [Chinese version](https://github.com/qubit-ltd/rs-command/blob/main/doc/user_guide.zh_CN.md)
/// for ownership details.
#[derive(Clone)]
pub struct CommandExecutor {
    /// Stores the manager value used by this execution component.
    pub(super) manager: qubit_operation::OperationManager,
    /// Stores the config value used by this execution component.
    pub(super) config: CommandExecutionConfig,
    /// Stores the scheduler value used by this execution component.
    pub(crate) scheduler: std::sync::Arc<scheduler::BoundedScheduler>,
    /// Keeps scheduler admission open while an external executor clone exists.
    _lifetime: std::sync::Arc<ExecutorLifetime>,
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
        let scheduler = std::sync::Arc::new(scheduler);
        let lifetime = std::sync::Arc::new(ExecutorLifetime::new(std::sync::Arc::clone(&scheduler)));
        Ok(Self {
            manager,
            config,
            scheduler,
            _lifetime: lifetime,
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

#[cfg(test)]
mod tests {
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::CommandExecutionConfig;
    use super::CommandExecutor;

    #[test]
    fn test_executor_exposes_its_config_and_manager_for_future_operations() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let config = CommandExecutionConfig {
            max_active: 2,
            max_queued: 3,
            max_waiters: 5,
            max_queued_input_bytes: 128,
            max_diagnostics: 7,
        };
        let executor = CommandExecutor::new(manager, config.clone()).unwrap();

        assert_eq!(executor.config(), &config);
        let outcome = executor
            .manager()
            .run(OperationOptions::default(), |_| Ok::<_, ()>("manager view works"))
            .unwrap();
        assert!(matches!(outcome, OperationOutcome::Succeeded("manager view works")));
    }
}
