#[cfg(feature = "async")]
use std::future::Future;
#[cfg(feature = "async")]
use std::sync::Arc;
#[cfg(feature = "async")]
use std::task::Context;
#[cfg(feature = "async")]
use std::task::Poll;
#[cfg(feature = "async")]
use std::task::Wake;
#[cfg(feature = "async")]
use std::task::Waker;
#[cfg(feature = "async")]
use std::thread;
use std::time::Duration;

#[cfg(feature = "async")]
use qubit_command::AsyncCommandOperation;
use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
#[cfg(feature = "async")]
use qubit_command::CommandExecutorApi;
use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunner;
use qubit_command::FileSinkPolicy;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;
use qubit_operation::SettlementStatus;

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
    use std::io::Write;
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().write_all(b"preserve this").unwrap();
    let output = std::fs::File::open(file.path()).unwrap();
    let options = CommandResourceRunOptions::new().stdout_sink(output, FileSinkPolicy::Truncate);
    let mut operation = executor
        .submit_resources(
            CommandRunner::new(Duration::from_secs(5)),
            Command::new("echo").arg("not-started"),
            options,
        )
        .unwrap();
    assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
    let settlement = operation.wait_settlement().unwrap();
    let external = settlement
        .external_settlement
        .as_ref()
        .expect("adapter settlement is retained by manager");
    assert_eq!(external.status, SettlementStatus::RecoveryRequired);
    assert_eq!(external.recovery_count, 1);
    assert_eq!(
        external.effects_total, 0,
        "scope effects are not copied into the domain report"
    );
    assert_eq!(external.effects.len(), 0);
    assert_eq!(
        external.recoveries_total, 0,
        "adapter owners are not manager RecoveryHandles"
    );
    assert!(!external.recoveries_truncated);
    assert_eq!(settlement.resource_reports_total, 1);
    assert_eq!(settlement.effects_total, 1);
    assert_eq!(settlement.effects[0].fact, qubit_operation::EffectFact::Unknown);
    assert!(settlement.effects[0].resource_id.is_some());
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
    let _outcome = operation.wait_outcome().unwrap();
}

#[cfg(unix)]
#[test]
fn shutdown_drain_returns_pending_then_completed_report() {
    use qubit_operation::ShutdownMode;
    use qubit_operation::ShutdownWait;
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut operation = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("sleep 0.2"),
            Default::default(),
        )
        .unwrap();
    let shutdown = executor.request_shutdown(ShutdownMode::Drain);
    assert!(matches!(
        shutdown.wait_for(Duration::ZERO).unwrap(),
        ShutdownWait::Pending(snapshot) if snapshot.active_operations == 1
    ));
    let _outcome = operation.wait_outcome().unwrap();
    operation.wait_settlement().unwrap();
    let report = match shutdown.wait_for(Duration::from_secs(5)).unwrap() {
        ShutdownWait::Completed(report) => report,
        ShutdownWait::Pending(snapshot) => panic!("drained group still pending: {snapshot:?}"),
    };
    assert_eq!(report.snapshot.active_operations, 0);
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
    assert!(matches!(second_outcome.unwrap(), OperationOutcome::Succeeded(_)));
    assert!(second_settlement.all_released());
}
