// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(all(feature = "async", unix))]
use std::future::Future;
#[cfg(all(feature = "async", unix))]
use std::sync::Arc;
#[cfg(all(feature = "async", unix))]
use std::task::Context;
#[cfg(all(feature = "async", unix))]
use std::task::Poll;
#[cfg(all(feature = "async", unix))]
use std::task::Wake;
#[cfg(all(feature = "async", unix))]
use std::task::Waker;
#[cfg(all(feature = "async", unix))]
use std::thread;
use std::time::Duration;

#[cfg(all(feature = "async", unix))]
use qubit_command::AsyncCommandOperation;
use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
#[cfg(all(feature = "async", unix))]
use qubit_command::CommandExecutorApi;
use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
use qubit_command::FileSinkPolicy;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;
use qubit_operation::SettlementStatus;

#[test]
fn test_managed_submit_runs_through_operation_manager() {
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

#[cfg(unix)]
#[test]
fn test_dropping_executor_keeps_accepted_operation_running_to_completion() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let started = directory.path().join("started");
    let release = directory.path().join("release");
    let script = "touch \"$0\"; while [ ! -f \"$1\" ]; do sleep 0.01; done; printf drained";
    let mut operation = executor
        .submit(
            CommandRunner::new(Duration::from_secs(5)),
            Command::new("sh")
                .arg("-c")
                .arg(script)
                .arg(started.to_str().unwrap())
                .arg(release.to_str().unwrap()),
            Default::default(),
        )
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !started.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !started.exists() {
        std::fs::write(&release, b"release").unwrap();
        let outcome = operation.wait_outcome();
        panic!("accepted operation should start before dropping the executor: {outcome:?}");
    }

    drop(executor);
    std::fs::write(&release, b"release").unwrap();

    match operation.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"drained"),
        other => panic!("accepted work should complete after executor drop: {other:?}"),
    }
    assert!(operation.wait_settlement().unwrap().all_released());
}

#[test]
fn test_managed_truncation_failure_is_typed_and_releases_registered_sink() {
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
    let OperationOutcome::Failed(error) = operation.wait_outcome().unwrap() else {
        panic!("read-only managed sink truncation must fail");
    };
    assert_eq!(error.kind(), qubit_command::CommandErrorKind::FilePreparationFailed);
    assert!(matches!(
        error.reason(),
        qubit_command::CommandErrorReason::FilePreparationFailed {
            stage: qubit_command::FilePreparationStage::Truncation,
            direction: Some(qubit_command::FilePreparationDirection::Stdout),
            ..
        }
    ));
    let settlement = operation.wait_settlement().unwrap();
    let external = settlement
        .external_settlement
        .as_ref()
        .expect("adapter settlement is retained by manager");
    assert_eq!(external.status, SettlementStatus::Released);
    assert_eq!(external.recovery_count, 0);
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

#[cfg(unix)]
#[test]
fn test_owned_file_preparation_failure_retains_recovery_settlement() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let input = std::fs::File::open(directory.path()).unwrap();
    let output = tempfile::NamedTempFile::new().unwrap();
    let sink = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .unwrap();
    let mut operation = executor
        .submit_resources(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("exit 0"),
            CommandResourceRunOptions::new()
                .stdin_file(input)
                .stdout_sink(sink, FileSinkPolicy::Preserve),
        )
        .unwrap();

    let OperationOutcome::Failed(error) = operation.wait_outcome().unwrap() else {
        panic!("a directory cannot be prepared as owned stdin");
    };
    assert_eq!(error.kind(), qubit_command::CommandErrorKind::FilePreparationFailed);
    assert!(matches!(
        error.reason(),
        qubit_command::CommandErrorReason::FilePreparationFailed {
            stage: qubit_command::FilePreparationStage::Validation,
            direction: Some(qubit_command::FilePreparationDirection::Stdin),
            ..
        }
    ));

    let settlement = operation.wait_settlement().unwrap();
    assert_eq!(settlement.status, SettlementStatus::RecoveryRequired);
    let external = settlement
        .external_settlement
        .as_ref()
        .expect("adapter recovery evidence is retained by manager");
    assert_eq!(external.status, SettlementStatus::RecoveryRequired);
    assert_eq!(external.recovery_count, 1);
    assert!(
        external.recoveries.is_empty(),
        "adapter recovery is not a manager RecoveryHandle"
    );
}

#[cfg(unix)]
#[test]
fn test_conflicting_owned_sink_and_path_tee_do_not_truncate_before_rejection() {
    for (owned_stream, tee_stream) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let owned = directory.path().join("owned");
        let tee = directory.path().join("tee");
        let started = directory.path().join("started");
        std::fs::write(&owned, b"preserve me").unwrap();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&owned).unwrap();
        let options = CommandRunOptions::new();
        let options = if tee_stream == 0 {
            options.tee_stdout_to_file(&tee)
        } else {
            options.tee_stderr_to_file(&tee)
        };
        let resources = if owned_stream == 0 {
            CommandResourceRunOptions::new()
                .options(options)
                .stdout_sink(file, FileSinkPolicy::Truncate)
        } else {
            CommandResourceRunOptions::new()
                .options(options)
                .stderr_sink(file, FileSinkPolicy::Truncate)
        };
        let mut operation = executor
            .submit_resources(
                CommandRunner::new(Duration::from_secs(5)),
                Command::new("sh")
                    .arg("-c")
                    .arg("touch \"$1\"")
                    .arg("managed-conflict-test")
                    .arg(started.to_str().unwrap()),
                resources,
            )
            .unwrap();

        assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        assert_eq!(std::fs::read(&owned).unwrap(), b"preserve me");
        assert!(!started.exists(), "the rejected command must not start");
        assert!(!tee.exists(), "the path tee must not be opened");
    }
}

#[cfg(unix)]
#[test]
fn test_managed_file_preparation_errors_report_stage_and_stream() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut operation = executor
        .submit_resources(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("exit 0"),
            CommandResourceRunOptions::new().stdin_file(std::fs::File::open(directory.path()).unwrap()),
        )
        .unwrap();
    let OperationOutcome::Failed(error) = operation.wait_outcome().unwrap() else {
        panic!("a directory cannot be used as managed stdin");
    };
    assert_eq!(error.kind(), qubit_command::CommandErrorKind::FilePreparationFailed);
    assert!(matches!(
        error.reason(),
        qubit_command::CommandErrorReason::FilePreparationFailed {
            stage: qubit_command::FilePreparationStage::Validation,
            direction: Some(qubit_command::FilePreparationDirection::Stdin),
            source,
        } if source.kind() == std::io::ErrorKind::InvalidInput
    ));
    assert!(!format!("{error:?}").contains("must refer to a regular file"));
}

#[cfg(unix)]
#[test]
fn test_managed_stderr_truncation_failure_is_typed_and_stream_specific() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let output = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(output.path(), b"keep stderr").unwrap();
    let output_path = output.path().to_path_buf();
    let stderr = std::fs::File::open(output.path()).unwrap();
    let mut operation = executor
        .submit_resources(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("exit 0"),
            CommandResourceRunOptions::new().stderr_sink(stderr, FileSinkPolicy::Truncate),
        )
        .unwrap();
    let OperationOutcome::Failed(error) = operation.wait_outcome().unwrap() else {
        panic!("a read-only stderr sink cannot be truncated");
    };
    assert_eq!(error.kind(), qubit_command::CommandErrorKind::FilePreparationFailed);
    assert!(matches!(
        error.reason(),
        qubit_command::CommandErrorReason::FilePreparationFailed {
            stage: qubit_command::FilePreparationStage::Truncation,
            direction: Some(qubit_command::FilePreparationDirection::Stderr),
            ..
        }
    ));
    assert_eq!(std::fs::read(output_path).unwrap(), b"keep stderr");
}

#[cfg(unix)]
#[test]
fn test_managed_commit_truncates_each_sink_once_and_keeps_stdin_cursor() {
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(b"prefix-payload").unwrap();
    input.seek(SeekFrom::Start(7)).unwrap();
    let stdout = tempfile::NamedTempFile::new().unwrap();
    let stderr = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(stdout.path(), b"old stdout").unwrap();
    std::fs::write(stderr.path(), b"old stderr").unwrap();
    let stdout_path = stdout.path().to_path_buf();
    let stderr_path = stderr.path().to_path_buf();
    let stdout = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&stdout_path)
        .unwrap();
    let stderr = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&stderr_path)
        .unwrap();
    let mut operation = executor
        .submit_resources(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("cat; printf error >&2"),
            CommandResourceRunOptions::new()
                .stdin_file(input)
                .stdout_sink(stdout, FileSinkPolicy::Truncate)
                .stderr_sink(stderr, FileSinkPolicy::Truncate),
        )
        .unwrap();
    let OperationOutcome::Succeeded(output) = operation.wait_outcome().unwrap() else {
        panic!("managed owned file commit should run");
    };
    assert_eq!(output.stdout(), b"payload");
    assert_eq!(std::fs::read(stdout_path).unwrap(), b"payload");
    assert_eq!(std::fs::read(stderr_path).unwrap(), b"error");
    let settlement = operation.wait_settlement().unwrap();
    assert!(settlement.all_released());
    assert_eq!(settlement.effects_total, 2, "each output sink should truncate once");
    assert!(
        settlement
            .effects
            .iter()
            .all(|effect| effect.fact == qubit_operation::EffectFact::Known)
    );
}

#[test]
fn test_executor_shutdown_rejection_returns_owned_options() {
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
fn test_executor_shutdown_isolated_to_its_group() {
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
fn test_shutdown_drain_returns_pending_then_completed_report() {
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
fn test_async_wait_future_is_lazy_and_cancelled_waiter_is_removed() {
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

#[cfg(all(feature = "async", unix))]
#[test]
fn test_async_submission_entry_points_complete_and_settle() {
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
        let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            thread::park_timeout(Duration::from_secs(2));
        }
    }

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut operation = CommandExecutorApi::submit(
        &executor,
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf trait-submit"),
        Default::default(),
    )
    .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"trait-submit"),
        other => panic!("trait submission failed: {other:?}"),
    }
    assert!(operation.wait_settlement().unwrap().all_released());

    let mut operation = CommandExecutorApi::submit_wait_blocking(
        &executor,
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf trait-wait"),
        Default::default(),
    )
    .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"trait-wait"),
        other => panic!("trait wait submission failed: {other:?}"),
    }
    assert!(operation.wait_settlement().unwrap().all_released());

    use std::io::Read;
    use std::io::Seek;
    let mut sync_output_file = tempfile::tempfile().unwrap();
    let mut operation = CommandExecutorApi::submit_resources(
        &executor,
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf trait-resource"),
        CommandResourceRunOptions::new().stdout_sink(sync_output_file.try_clone().unwrap(), FileSinkPolicy::Truncate),
    )
    .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    assert!(operation.wait_settlement().unwrap().all_released());
    sync_output_file.rewind().unwrap();
    let mut written = String::new();
    sync_output_file.read_to_string(&mut written).unwrap();
    assert_eq!(written, "trait-resource");

    let mut operation = CommandExecutorApi::submit_async(
        &executor,
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf async-output"),
        Default::default(),
    )
    .unwrap();
    let outcome = block_on(operation.wait_outcome()).unwrap();
    let settlement = block_on(operation.wait_settlement());
    match outcome {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"async-output"),
        other => panic!("async submission failed: {other:?}"),
    }
    assert!(settlement.all_released());

    let mut output_file = tempfile::tempfile().unwrap();
    let operation = CommandExecutorApi::submit_async_resources(
        &executor,
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf resource-output"),
        CommandResourceRunOptions::new().stdout_sink(output_file.try_clone().unwrap(), FileSinkPolicy::Truncate),
    )
    .unwrap();
    let outcome = block_on(async move {
        let mut operation = operation;
        let outcome = operation.wait_outcome().await;
        let settlement = operation.wait_settlement().await;
        (outcome, settlement)
    });
    assert!(matches!(outcome.0.unwrap(), OperationOutcome::Succeeded(_)));
    assert!(outcome.1.all_released());
    output_file.rewind().unwrap();
    let mut written = String::new();
    output_file.read_to_string(&mut written).unwrap();
    assert_eq!(written, "resource-output");
}
