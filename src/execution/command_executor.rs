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

// Owns private state types used by executor admission and waiting.
#[path = "command_executor/internal/mod.rs"]
pub(super) mod internal;
// Owns the public executor snapshot and shutdown operations.
#[path = "command_executor/lifecycle.rs"]
mod lifecycle;
// Owns the executor's inherent submission API and implementations.
#[path = "command_executor/submission.rs"]
pub(super) mod submission;
// Owns the executor's retry-wait implementation and waiter lifecycle.
#[path = "command_executor/wait.rs"]
pub(super) mod wait;
// Owns the terminal results shared by retry-wait paths.
#[path = "command_executor/wait_result.rs"]
mod wait_result;

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
///
/// # Example
///
/// Construct an executor through the public API:
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let manager = qubit_operation::OperationManager::new(Default::default())?;
/// let executor = qubit_command::CommandExecutor::new(
///     manager,
///     qubit_command::CommandExecutionConfig::default(),
/// )?;
/// assert_eq!(executor.config().max_active, 4);
/// # Ok(())
/// # }
/// ```
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
    /// Creates an executor with validated local capacity limits.
    ///
    /// # Parameters
    /// `manager` owns the operation group used for submissions. `config`
    /// sets local active, queued, waiter, and input-byte limits. `max_waiters`
    /// covers both capacity waiters and callers retrying manager `Busy`.
    /// `max_diagnostics` is retained as a compatibility field and has no
    /// effect.
    /// Accepted operations retain their reservations until settlement and
    /// resource recovery release them.
    ///
    /// # Errors
    /// Returns [`CommandExecutorCreateError`] if the configuration is invalid
    /// or the executor's managed workers cannot be started.
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
