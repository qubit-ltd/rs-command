#[cfg(feature = "async")]
use qubit_command::{AsyncCommandOperation, CommandExecutorApi};
use qubit_command::{
    Command, CommandExecutionConfig, CommandExecutor, CommandResourceRunOptions, CommandRunner,
    FileSinkPolicy,
};
use qubit_operation::{ManagerOptions, OperationManager, OperationOutcome, SettlementStatus};
use std::time::Duration;
#[cfg(feature = "async")]
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};

#[test]
fn managed_submit_runs_through_operation_manager() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut operation = executor
        .submit(
            CommandRunner::new(Duration::from_secs(5)),
            Command::new("echo").arg("managed"),
            Default::default(),
        )
        .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => {
            assert!(String::from_utf8_lossy(output.stdout()).contains("managed"))
        }
        other => panic!("unexpected managed outcome: {other:?}"),
    }
}

#[test]
fn owned_preparation_failure_retains_and_reports_recovery() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let file = tempfile::tempfile().unwrap();
    let output = file.try_clone().unwrap();
    let options = CommandResourceRunOptions::new()
        .stdin_file(file)
        .stdout_sink(output, FileSinkPolicy::Preserve);
    let mut operation = executor
        .submit_resources(
            CommandRunner::new(Duration::from_secs(5)),
            Command::new("echo").arg("not-started"),
            options,
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Failed(_)
    ));
    let settlement = operation.wait_settlement().unwrap();
    let external = settlement
        .external_settlement
        .as_ref()
        .expect("adapter settlement is retained by manager");
    assert_eq!(external.status, SettlementStatus::RecoveryRequired);
    assert_eq!(external.recovery_count, 1);
    assert_eq!(external.effects_total, 1);
    assert!(
        external.recoveries_truncated,
        "opaque owner is retained even when public recovery-handle detail cannot be materialized"
    );
}

#[test]
fn executor_shutdown_rejection_returns_owned_options() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Drain);
    let file = tempfile::tempfile().unwrap();
    let failure = match executor.submit_resources(
        CommandRunner::new(Duration::from_secs(5)),
        Command::new("echo"),
        CommandResourceRunOptions::new().stdin_file(file),
    ) {
        Err(failure) => failure,
        Ok(_) => panic!("closed manager unexpectedly accepted the owned request"),
    };
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(_)
    ));
    assert!(failure.options.configured_stdin().is_some());
}

#[test]
fn executor_shutdown_isolated_to_its_group() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let first = CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).unwrap();
    let second = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let _shutdown = first.request_shutdown(qubit_operation::ShutdownMode::Drain);
    let mut operation = second
        .submit(
            CommandRunner::new(Duration::from_secs(5)),
            Command::new("echo").arg("still-available"),
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
}

#[cfg(all(feature = "async", unix))]
#[test]
fn async_wait_future_is_lazy_and_cancelled_waiter_is_removed() {
    struct ThreadWake(thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark()
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark()
        }
    }
    fn block_on<F: Future>(future: F) -> F::Output {
        let w = Waker::from(Arc::new(ThreadWake(thread::current())));
        let mut cx = Context::from_waker(&w);
        let mut f = Box::pin(future);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
            thread::park_timeout(Duration::from_secs(2));
        }
    }
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = CommandExecutionConfig {
        max_active: 1,
        max_queued: 0,
        max_waiters: 2,
        ..Default::default()
    };
    let executor = CommandExecutor::new(manager, config).unwrap();
    let mut first = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("sleep 0.15"),
            Default::default(),
        )
        .unwrap();
    let lazy = executor.submit_wait(
        CommandRunner::without_timeout(),
        Command::new("echo").arg("later"),
        Default::default(),
    );
    assert_eq!(executor.scheduler_snapshot().active, 1);
    drop(lazy);
    assert_eq!(executor.scheduler_snapshot().waiters, 0);
    let waiting = executor.submit_wait(
        CommandRunner::without_timeout(),
        Command::new("echo").arg("later"),
        Default::default(),
    );
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut waiting = Box::pin(waiting);
    assert!(waiting.as_mut().poll(&mut cx).is_pending());
    assert_eq!(executor.scheduler_snapshot().waiters, 1);
    drop(waiting);
    assert_eq!(executor.scheduler_snapshot().waiters, 0);
    let observe_first = thread::spawn(move || {
        let outcome = first.wait_outcome().unwrap();
        let settlement = first.wait_settlement().unwrap();
        (outcome, settlement)
    });
    let second: AsyncCommandOperation = block_on(executor.submit_wait(
        CommandRunner::without_timeout(),
        Command::new("echo").arg("later"),
        Default::default(),
    ))
    .unwrap();
    let (first_outcome, first_settlement) = observe_first.join().unwrap();
    assert!(matches!(first_outcome, OperationOutcome::Succeeded(_)));
    assert!(first_settlement.all_released());
    let (second_outcome, second_settlement) = block_on({
        let mut h = second;
        async move {
            let outcome = h.wait_outcome().await;
            let settlement = h.wait_settlement().await;
            (outcome, settlement)
        }
    });
    assert!(matches!(
        second_outcome.unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    assert!(second_settlement.all_released());
}
