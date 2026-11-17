// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandExecutorApi;
use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunner;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;

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
