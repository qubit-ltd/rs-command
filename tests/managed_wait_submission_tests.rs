// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use qubit_command::Command;
#[cfg(unix)]
use qubit_command::CommandExecutionConfig;
#[cfg(unix)]
use qubit_command::CommandExecutor;
#[cfg(unix)]
use qubit_command::CommandExecutorApi;
#[cfg(unix)]
use qubit_command::CommandResourceRunOptions;
#[cfg(unix)]
use qubit_command::CommandRunner;
#[cfg(unix)]
use qubit_operation::ManagerOptions;
#[cfg(unix)]
use qubit_operation::OperationManager;
#[cfg(unix)]
use qubit_operation::OperationOptions;
#[cfg(unix)]
use qubit_operation::OperationOutcome;

#[cfg(unix)]
fn new_executor() -> CommandExecutor {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    CommandExecutor::new(
        manager,
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 2,
            ..Default::default()
        },
    )
    .unwrap()
}

#[cfg(unix)]
fn occupying_command(executor: &CommandExecutor) -> qubit_command::CommandOperation {
    executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("sleep 0.15; printf first"),
            Default::default(),
        )
        .unwrap()
}

#[cfg(unix)]
#[test]
fn test_blocking_wait_submission_runs_after_active_slot_is_released() {
    let executor = new_executor();
    let mut first = occupying_command(&executor);
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let submitter = std::thread::spawn(move || {
        let operation = waiting_executor
            .submit_wait_blocking(
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("printf second"),
                Default::default(),
            )
            .unwrap();
        tx.send(operation).unwrap();
    });

    assert!(matches!(first.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
    let mut second = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("wait submission should resume");
    submitter.join().unwrap();
    match second.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"second"),
        other => panic!("waiting submission failed: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn test_accepted_queued_command_dispatches_after_active_slot_is_released() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(
        manager,
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let mut active = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("sleep 0.15; printf active"),
            Default::default(),
        )
        .unwrap();
    let mut queued = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("printf queued"),
            Default::default(),
        )
        .expect("second command should be accepted into the queue");
    assert_eq!(executor.scheduler_snapshot().queued, 1);

    match active.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"active"),
        other => panic!("active command failed: {other:?}"),
    }
    assert!(active.wait_settlement().unwrap().all_released());
    match queued.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"queued"),
        other => panic!("queued command failed: {other:?}"),
    }
    assert!(queued.wait_settlement().unwrap().all_released());
}

#[cfg(unix)]
#[test]
fn test_owned_wait_submission_preserves_input_until_capacity_is_available() {
    use std::io::Seek;
    use std::io::Write;

    let executor = new_executor();
    let mut first = occupying_command(&executor);
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(b"queued-input").unwrap();
    input.rewind().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let submitter = std::thread::spawn(move || {
        let operation = waiting_executor
            .submit_wait_resources_blocking(
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("cat"),
                CommandResourceRunOptions::new().stdin_file(input),
            )
            .unwrap();
        tx.send(operation).unwrap();
    });

    assert!(matches!(first.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
    let mut second = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("owned wait submission should resume");
    submitter.join().unwrap();
    match second.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"queued-input"),
        other => panic!("owned waiting submission failed: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn test_scoped_blocking_wait_rejects_foreign_manager_scope() {
    let executor = new_executor();
    let foreign_manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let outcome = foreign_manager
        .run(qubit_operation::OperationOptions::default(), move |scope| {
            let failure = match executor.submit_wait_blocking_in(
                &scope,
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("printf should-not-run"),
                Default::default(),
            ) {
                Err(failure) => failure,
                Ok(_) => panic!("a foreign operation scope must be rejected"),
            };
            assert!(matches!(
                failure.reason,
                qubit_command::CommandSubmitFailureReason::GroupBinding(_)
            ));
            Ok::<_, ()>(())
        })
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Succeeded(())));
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_scoped_wait_api_entry_points_complete_and_settle() {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
            std::thread::park_timeout(Duration::from_secs(2));
        }
    }

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).unwrap();
    let result = manager
        .run(OperationOptions::default(), move |scope| {
            let mut standard = block_on(<CommandExecutor as CommandExecutorApi>::submit_wait_in(
                &executor,
                &scope,
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("printf scoped-wait"),
                Default::default(),
            ))
            .expect("scoped standard wait should submit");
            assert!(matches!(
                block_on(standard.wait_outcome()).expect("standard operation should finish"),
                OperationOutcome::Succeeded(_)
            ));
            assert!(block_on(standard.wait_settlement()).all_released());

            let mut resources = block_on(<CommandExecutor as CommandExecutorApi>::submit_wait_resources_in(
                &executor,
                &scope,
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("printf scoped-resource-wait"),
                CommandResourceRunOptions::new(),
            ))
            .expect("scoped resource wait should submit");
            assert!(matches!(
                block_on(resources.wait_outcome()).expect("resource operation should finish"),
                OperationOutcome::Succeeded(_)
            ));
            assert!(block_on(resources.wait_settlement()).all_released());

            let mut submitted_resources = <CommandExecutor as CommandExecutorApi>::submit_resources_in(
                &executor,
                &scope,
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("printf scoped-resource-submit"),
                CommandResourceRunOptions::new(),
            )
            .expect("scoped resource submission should succeed");
            assert!(matches!(
                submitted_resources
                    .wait_outcome()
                    .expect("resource submission should finish"),
                OperationOutcome::Succeeded(_)
            ));
            assert!(
                submitted_resources
                    .wait_settlement()
                    .expect("resource submission should settle")
                    .all_released()
            );

            let mut blocking_resources = <CommandExecutor as CommandExecutorApi>::submit_wait_resources_blocking_in(
                &executor,
                &scope,
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("printf scoped-blocking-resource-wait"),
                CommandResourceRunOptions::new(),
            )
            .expect("scoped blocking resource wait should succeed");
            assert!(matches!(
                blocking_resources
                    .wait_outcome()
                    .expect("blocking resource wait should finish"),
                OperationOutcome::Succeeded(_)
            ));
            assert!(
                blocking_resources
                    .wait_settlement()
                    .expect("blocking resource wait should settle")
                    .all_released()
            );
            Ok::<_, ()>(())
        })
        .unwrap();
    assert!(matches!(result, OperationOutcome::Succeeded(())));
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_async_owned_wait_submission_resumes_after_capacity_release() {
    use std::future::Future;
    use std::io::Seek;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
            std::thread::park_timeout(Duration::from_secs(2));
        }
    }

    let executor = new_executor();
    let mut first = occupying_command(&executor);
    let mut input = tempfile::tempfile().unwrap();
    use std::io::Write;
    input.write_all(b"async-queued-input").unwrap();
    input.rewind().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let waiter = std::thread::spawn(move || {
        let operation = block_on(waiting_executor.submit_wait_resources(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("cat"),
            CommandResourceRunOptions::new().stdin_file(input),
        ))
        .unwrap();
        tx.send(operation).unwrap();
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "async wait should register capacity waiter"
        );
        std::thread::yield_now();
    }
    assert!(matches!(first.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
    let mut second = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("async submission should resume");
    waiter.join().unwrap();
    match block_on(second.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"async-queued-input"),
        other => panic!("async owned waiting submission failed: {other:?}"),
    }
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_async_wait_submissions_report_closed_after_shutdown_wakes_waiters() {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let executor = new_executor();
    let mut first = occupying_command(&executor);
    let mut waiting = Box::pin(executor.submit_wait(
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf should-not-run"),
        Default::default(),
    ));
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(waiting.as_mut().poll(&mut context), Poll::Pending));
    assert_eq!(executor.scheduler_snapshot().waiters, 1);
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    assert!(matches!(
        waiting.as_mut().poll(&mut context),
        Poll::Ready(Err(failure))
            if matches!(failure.reason, qubit_command::CommandSubmitFailureReason::Capacity(
                qubit_command::execution::CapacityFailure::Closed
            ))
    ));
    let _ = first.wait_outcome();

    let executor = new_executor();
    let mut first = occupying_command(&executor);
    let mut waiting = Box::pin(executor.submit_wait_resources(
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf should-not-run"),
        CommandResourceRunOptions::new(),
    ));
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(waiting.as_mut().poll(&mut context), Poll::Pending));
    assert_eq!(executor.scheduler_snapshot().waiters, 1);
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    assert!(matches!(
        waiting.as_mut().poll(&mut context),
        Poll::Ready(Err(failure))
            if matches!(failure.reason, qubit_command::CommandSubmitFailureReason::Capacity(
                qubit_command::execution::CapacityFailure::Closed
            ))
    ));
    let _ = first.wait_outcome();
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_scoped_async_resource_wait_returns_owned_input_for_foreign_scope() {
    use std::future::Future;
    use std::io::Read;
    use std::io::Seek;
    use std::io::Write;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
            std::thread::park_timeout(Duration::from_secs(2));
        }
    }

    let executor = new_executor();
    let foreign_manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let outcome = foreign_manager
        .run(qubit_operation::OperationOptions::default(), move |scope| {
            let mut input = tempfile::tempfile().unwrap();
            input.write_all(b"preserved-after-rejection").unwrap();
            input.rewind().unwrap();
            let failure = match block_on(executor.submit_wait_resources_in(
                &scope,
                CommandRunner::without_timeout(),
                Command::new("cat"),
                CommandResourceRunOptions::new().stdin_file(input),
            )) {
                Err(failure) => failure,
                Ok(_) => panic!("a foreign operation scope must be rejected"),
            };
            assert!(matches!(
                failure.reason,
                qubit_command::CommandSubmitFailureReason::GroupBinding(_)
            ));
            let mut returned = failure
                .options
                .configured_stdin()
                .expect("rejected options must retain their stdin file");
            let mut bytes = Vec::new();
            returned.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"preserved-after-rejection");
            Ok::<_, ()>(())
        })
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Succeeded(())));
}
