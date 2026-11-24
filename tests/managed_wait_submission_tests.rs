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
use qubit_command::CommandRunOptions;
#[cfg(unix)]
use qubit_command::CommandRunner;
#[cfg(unix)]
use qubit_operation::ManagerOptions;
#[cfg(unix)]
use qubit_operation::OperationManager;
#[cfg(all(feature = "async", unix))]
use qubit_operation::OperationOptions;
#[cfg(unix)]
use qubit_operation::OperationOutcome;

mod support;

#[cfg(all(feature = "async", unix))]
fn block_on_wait<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::Context;
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

    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park_timeout(Duration::from_secs(2));
    }
}

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
fn new_manager_busy_executor() -> (OperationManager, CommandExecutor) {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(
        manager.clone(),
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 2,
            ..Default::default()
        },
    )
    .unwrap();
    (manager, executor)
}

#[cfg(unix)]
fn hold_executor_manager_group(
    executor: &CommandExecutor,
) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let manager = executor.manager().clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _ = manager
            .run(qubit_operation::OperationOptions::default(), move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
    });
    started_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("bound manager group operation should start");
    (release_tx, holder)
}

#[cfg(unix)]
fn wait_for_manager_busy_waiter(executor: &CommandExecutor) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "manager Busy wait did not register"
        );
        std::thread::yield_now();
    }
    assert_eq!(executor.scheduler_snapshot().active, 0);
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
fn test_blocking_wait_submission_keeps_the_first_io_path_base_after_cwd_change() {
    const TEST_NAME: &str = "test_blocking_wait_submission_keeps_the_first_io_path_base_after_cwd_change";
    if std::env::var_os("QUBIT_COMMAND_MANAGED_TEST_CHILD").is_none() {
        support::managed_test_process::run_for(TEST_NAME, None, Duration::from_secs(20), || {})
            .expect("isolated path snapshot test should pass");
        return;
    }

    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    std::fs::write(first.path().join("input.txt"), b"first-directory-input").unwrap();
    std::fs::write(second.path().join("input.txt"), b"second-directory-input").unwrap();
    std::env::set_current_dir(first.path()).unwrap();

    let executor = new_executor();
    let mut active = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("sleep 0.5"),
            Default::default(),
        )
        .unwrap();
    let waiting_executor = executor.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let result = waiting_executor.submit_wait_blocking(
            CommandRunner::without_timeout(),
            Command::new("sh")
                .arg("-c")
                .arg("cat; printf stderr-marker >&2")
                .stdin_file("input.txt"),
            CommandRunOptions::new()
                .tee_stdout_to_file("stdout.log")
                .tee_stderr_to_file("stderr.log"),
        );
        tx.send(result).unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "waiter should register before cwd changes"
        );
        std::thread::yield_now();
    }
    std::env::set_current_dir(second.path()).unwrap();
    let _ = active.wait_outcome().unwrap();
    let mut operation = rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
    waiter.join().unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(output) => {
            assert_eq!(output.stdout(), b"first-directory-input")
        }
        other => panic!("waiting command should succeed: {other:?}"),
    }
    assert_eq!(
        std::fs::read(first.path().join("stdout.log")).unwrap(),
        b"first-directory-input"
    );
    assert_eq!(
        std::fs::read(first.path().join("stderr.log")).unwrap(),
        b"stderr-marker"
    );
    assert!(!second.path().join("stdout.log").exists());
    assert!(!second.path().join("stderr.log").exists());
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_async_wait_submission_captures_io_base_on_first_poll() {
    const TEST_NAME: &str = "test_async_wait_submission_captures_io_base_on_first_poll";
    if std::env::var_os("QUBIT_COMMAND_MANAGED_TEST_CHILD").is_none() {
        support::managed_test_process::run_for(TEST_NAME, None, Duration::from_secs(20), || {})
            .expect("isolated async path snapshot test should pass");
        return;
    }

    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    std::fs::write(first.path().join("input.txt"), b"first-poll-input").unwrap();
    std::fs::write(second.path().join("input.txt"), b"second-poll-input").unwrap();
    std::env::set_current_dir(first.path()).unwrap();
    let executor = new_executor();
    let mut active = executor
        .submit(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("sleep 0.5"),
            Default::default(),
        )
        .unwrap();

    let waiting = executor.submit_wait(
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("cat").stdin_file("input.txt"),
        CommandRunOptions::new().tee_stdout_to_file("stdout.log"),
    );
    std::env::set_current_dir(second.path()).unwrap();
    let active_waiter = std::thread::spawn(move || active.wait_outcome().unwrap());
    let mut operation = block_on_wait(waiting).unwrap();
    let _ = active_waiter.join().unwrap();
    match block_on_wait(operation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"second-poll-input"),
        other => panic!("async waiting command should succeed: {other:?}"),
    }
    assert_eq!(
        std::fs::read(second.path().join("stdout.log")).unwrap(),
        b"second-poll-input"
    );
    assert!(!first.path().join("stdout.log").exists());
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_resource_wait_paths_keep_the_first_base_for_root_entries() {
    const TEST_NAME: &str = "test_resource_wait_paths_keep_the_first_base_for_root_entries";
    if std::env::var_os("QUBIT_COMMAND_MANAGED_TEST_CHILD").is_none() {
        support::managed_test_process::run_for(TEST_NAME, None, Duration::from_secs(30), || {})
            .expect("isolated resource path snapshot test should pass");
        return;
    }

    fn collect_sync(mut operation: qubit_command::CommandOperation) -> Vec<u8> {
        match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(output) => output.stdout().to_vec(),
            other => panic!("resource waiting command should succeed: {other:?}"),
        }
    }

    fn collect_async(mut operation: qubit_command::AsyncCommandOperation) -> Vec<u8> {
        match block_on_wait(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(output) => output.stdout().to_vec(),
            other => panic!("async resource waiting command should succeed: {other:?}"),
        }
    }

    fn run_case(asynchronous: bool) {
        let original_directory = std::env::current_dir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(first.path().join("input.txt"), b"first-resource-path").unwrap();
        std::fs::write(second.path().join("input.txt"), b"second-resource-path").unwrap();
        std::env::set_current_dir(first.path()).unwrap();

        let executor = new_executor();
        let mut active = executor
            .submit(
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("sleep 2"),
                Default::default(),
            )
            .unwrap();
        let waiting_executor = executor.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let submitter = std::thread::spawn(move || {
            let submit_and_collect = move || {
                let command = Command::new("sh")
                    .arg("-c")
                    .arg("cat; printf resource-stderr >&2")
                    .stdin_file("input.txt");
                let options = CommandRunOptions::new()
                    .tee_stdout_to_file("stdout.log")
                    .tee_stderr_to_file("stderr.log");
                let resources = CommandResourceRunOptions::new().options(options);
                match asynchronous {
                    false => collect_sync(
                        waiting_executor
                            .submit_wait_resources_blocking(CommandRunner::without_timeout(), command, resources)
                            .unwrap(),
                    ),
                    true => collect_async(
                        block_on_wait(waiting_executor.submit_wait_resources(
                            CommandRunner::without_timeout(),
                            command,
                            resources,
                        ))
                        .unwrap(),
                    ),
                }
            };
            tx.send(submit_and_collect()).unwrap();
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while executor.scheduler_snapshot().waiters == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "resource waiter should register: async={asynchronous}"
            );
            std::thread::yield_now();
        }
        std::env::set_current_dir(second.path()).unwrap();
        let _ = active.wait_outcome().unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), b"first-resource-path");
        submitter.join().unwrap();
        assert_eq!(
            std::fs::read(first.path().join("stdout.log")).unwrap(),
            b"first-resource-path"
        );
        assert_eq!(
            std::fs::read(first.path().join("stderr.log")).unwrap(),
            b"resource-stderr"
        );
        assert!(!second.path().join("stdout.log").exists());
        assert!(!second.path().join("stderr.log").exists());
        std::env::set_current_dir(original_directory).unwrap();
    }

    for asynchronous in [false, true] {
        run_case(asynchronous);
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
fn test_blocking_resource_wait_returns_closed_and_preserves_input_after_shutdown() {
    use std::io::Read;
    use std::io::Seek;
    use std::io::Write;

    let executor = new_executor();
    let mut first = occupying_command(&executor);
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(b"preserve-on-close").unwrap();
    input.rewind().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let submitter = std::thread::spawn(move || {
        tx.send(waiting_executor.submit_wait_resources_blocking(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("cat"),
            CommandResourceRunOptions::new().stdin_file(input),
        ))
        .unwrap();
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(std::time::Instant::now() < deadline, "blocking waiter did not register");
        std::thread::yield_now();
    }
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let failure = match rx
        .recv_timeout(Duration::from_secs(3))
        .expect("closed blocking wait should return")
    {
        Err(failure) => failure,
        Ok(_) => panic!("closed executor must reject the waiting submission"),
    };
    submitter.join().unwrap();
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    assert_eq!(failure.command.program(), "sh");
    let mut returned_input = failure
        .options
        .configured_stdin()
        .expect("rejected options retain the input file");
    let mut bytes = Vec::new();
    returned_input.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"preserve-on-close");
    let _ = first.wait_outcome();
}

#[cfg(unix)]
#[test]
fn test_blocking_standard_wait_returns_closed_after_shutdown() {
    let executor = new_executor();
    let mut active = occupying_command(&executor);
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let submitter = std::thread::spawn(move || {
        tx.send(waiting_executor.submit_wait_blocking(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("cat").stdin_file("relative-input.txt"),
            CommandRunOptions::new().tee_stdout_to_file("relative-output.log"),
        ))
        .unwrap();
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(std::time::Instant::now() < deadline, "blocking waiter did not register");
        std::thread::yield_now();
    }
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let failure = match rx
        .recv_timeout(Duration::from_secs(3))
        .expect("closed blocking wait should return")
    {
        Err(failure) => failure,
        Ok(_) => panic!("closed executor must reject the waiting submission"),
    };
    submitter.join().unwrap();
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    assert_eq!(
        failure.options.configured_stdout_file(),
        Some(std::path::Path::new("relative-output.log"))
    );
    let _ = active.wait_outcome();
}

#[cfg(unix)]
#[test]
fn test_scoped_blocking_resource_wait_returns_closed_after_shutdown() {
    use std::io::Read;
    use std::io::Seek;
    use std::io::Write;

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(
        manager.clone(),
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let mut active = occupying_command(&executor);
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let waiting_manager = manager.clone();
    let submitter = std::thread::spawn(move || {
        let _ = waiting_manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let mut input = tempfile::tempfile().unwrap();
                input.write_all(b"scoped-preserve-on-close").unwrap();
                input.rewind().unwrap();
                let failure = match waiting_executor.submit_wait_resources_blocking_in(
                    &scope,
                    CommandRunner::without_timeout(),
                    Command::new("sh").arg("-c").arg("cat"),
                    CommandResourceRunOptions::new().stdin_file(input),
                ) {
                    Err(failure) => failure,
                    Ok(_) => panic!("closed scoped executor must reject the waiter"),
                };
                let mut input = failure
                    .options
                    .configured_stdin()
                    .expect("rejected options retain the input file");
                let mut bytes = Vec::new();
                input.read_to_end(&mut bytes).unwrap();
                tx.send((failure.reason, bytes)).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(std::time::Instant::now() < deadline, "scoped waiter did not register");
        std::thread::yield_now();
    }
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let (reason, bytes) = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("closed scoped wait should return");
    submitter.join().unwrap();
    assert!(matches!(
        reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    assert_eq!(bytes, b"scoped-preserve-on-close");
    let _ = active.wait_outcome();
}

#[cfg(unix)]
#[test]
fn test_blocking_resource_wait_returns_closed_after_manager_busy() {
    use std::io::Read;
    use std::io::Seek;
    use std::io::Write;

    let (_, executor) = new_manager_busy_executor();
    let (release_tx, busy_operation) = hold_executor_manager_group(&executor);

    let mut input = tempfile::tempfile().unwrap();
    input.write_all(b"preserve-on-manager-busy-close").unwrap();
    input.rewind().unwrap();
    let waiting_executor = executor.clone();
    let (failure_tx, failure_rx) = std::sync::mpsc::channel();
    let submitter = std::thread::spawn(move || {
        failure_tx
            .send(waiting_executor.submit_wait_resources_blocking(
                CommandRunner::without_timeout(),
                Command::new("sh").arg("-c").arg("cat"),
                CommandResourceRunOptions::new().stdin_file(input),
            ))
            .unwrap();
    });

    wait_for_manager_busy_waiter(&executor);
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let failure = match failure_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("closed manager Busy wait should return")
    {
        Err(failure) => failure,
        Ok(_) => panic!("closed executor must reject the manager Busy waiter"),
    };
    submitter.join().unwrap();
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    let mut returned_input = failure
        .options
        .configured_stdin()
        .expect("rejected options retain the input file");
    let mut bytes = Vec::new();
    returned_input.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"preserve-on-manager-busy-close");
    release_tx.send(()).unwrap();
    busy_operation.join().unwrap();
}

#[cfg(unix)]
#[test]
fn test_blocking_standard_wait_returns_closed_after_manager_busy() {
    let (_, executor) = new_manager_busy_executor();
    let (release_tx, busy_operation) = hold_executor_manager_group(&executor);
    let waiting_executor = executor.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let submitter = std::thread::spawn(move || {
        tx.send(waiting_executor.submit_wait_blocking(
            CommandRunner::without_timeout(),
            Command::new("sh").arg("-c").arg("printf should-not-run"),
            Default::default(),
        ))
        .unwrap();
    });
    wait_for_manager_busy_waiter(&executor);
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let failure = match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        Err(failure) => failure,
        Ok(_) => panic!("closed executor must reject manager Busy waiter"),
    };
    submitter.join().unwrap();
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    release_tx.send(()).unwrap();
    busy_operation.join().unwrap();
}

#[cfg(unix)]
#[test]
fn test_scoped_blocking_waits_return_closed_after_manager_busy() {
    use std::io::Read;
    use std::io::Seek;
    use std::io::Write;

    for resource in [false, true] {
        let (manager, executor) = new_manager_busy_executor();
        let (release_tx, busy_operation) = hold_executor_manager_group(&executor);
        let waiting_executor = executor.clone();
        let waiting_manager = manager.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let submitter = std::thread::spawn(move || {
            let _ = waiting_manager
                .run(Default::default(), move |scope| {
                    let result: Result<(), (qubit_command::CommandSubmitFailureReason, Option<Vec<u8>>)> = if resource {
                        let mut input = tempfile::tempfile().unwrap();
                        input.write_all(b"scoped-manager-busy").unwrap();
                        input.rewind().unwrap();
                        waiting_executor
                            .submit_wait_resources_blocking_in(
                                &scope,
                                CommandRunner::without_timeout(),
                                Command::new("sh").arg("-c").arg("cat"),
                                CommandResourceRunOptions::new().stdin_file(input),
                            )
                            .map(|_| ())
                            .map_err(|f| {
                                let mut input = f.options.configured_stdin().unwrap();
                                let mut bytes = Vec::new();
                                input.read_to_end(&mut bytes).unwrap();
                                (f.reason, Some(bytes))
                            })
                    } else {
                        waiting_executor
                            .submit_wait_blocking_in(
                                &scope,
                                CommandRunner::without_timeout(),
                                Command::new("sh").arg("-c").arg("printf should-not-run"),
                                Default::default(),
                            )
                            .map(|_| ())
                            .map_err(|f| (f.reason, None))
                    };
                    tx.send(result).unwrap();
                    Ok::<_, ()>(())
                })
                .unwrap();
        });
        wait_for_manager_busy_waiter(&executor);
        let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
        let (reason, bytes) = match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            Err(failure) => failure,
            Ok(()) => panic!("closed scoped executor must reject manager Busy waiter"),
        };
        submitter.join().unwrap();
        assert!(matches!(
            reason,
            qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
        ));
        if resource {
            assert_eq!(bytes.unwrap(), b"scoped-manager-busy");
        }
        release_tx.send(()).unwrap();
        busy_operation.join().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn test_scoped_blocking_standard_wait_returns_closed_after_shutdown() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(
        manager.clone(),
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let mut active = occupying_command(&executor);
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let waiting_manager = manager.clone();
    let submitter = std::thread::spawn(move || {
        let _ = waiting_manager
            .run(qubit_operation::OperationOptions::default(), move |scope| {
                let failure = match waiting_executor.submit_wait_blocking_in(
                    &scope,
                    CommandRunner::without_timeout(),
                    Command::new("sh").arg("-c").arg("printf should-not-run"),
                    Default::default(),
                ) {
                    Err(failure) => failure,
                    Ok(_) => panic!("closed scoped executor must reject the waiter"),
                };
                tx.send(failure.reason).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(std::time::Instant::now() < deadline, "scoped waiter did not register");
        std::thread::yield_now();
    }
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let reason = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("closed scoped wait should return");
    submitter.join().unwrap();
    assert!(matches!(
        reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    let _ = active.wait_outcome();
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
fn test_async_resource_wait_returns_closed_after_manager_busy() {
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

    let executor = CommandExecutor::new(
        OperationManager::new(ManagerOptions::default()).unwrap(),
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let busy_manager = executor.manager().clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let busy_operation = std::thread::spawn(move || {
        let _ = busy_manager
            .run(qubit_operation::OperationOptions::default(), move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
    });
    started_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("bound group operation should start");

    let mut input = tempfile::tempfile().unwrap();
    input.write_all(b"async-preserve-on-manager-busy-close").unwrap();
    input.rewind().unwrap();
    let mut waiting = Box::pin(executor.submit_wait_resources(
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("cat"),
        CommandResourceRunOptions::new().stdin_file(input),
    ));
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(waiting.as_mut().poll(&mut context), Poll::Pending));
    assert_eq!(executor.scheduler_snapshot().active, 0);
    assert_eq!(executor.scheduler_snapshot().waiters, 1);

    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let failure = match waiting.as_mut().poll(&mut context) {
        Poll::Ready(Err(failure)) => failure,
        Poll::Ready(Ok(_)) => panic!("closed executor must reject the manager Busy waiter"),
        Poll::Pending => panic!("shutdown must finish the registered manager Busy wait"),
    };
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    let mut returned_input = failure
        .options
        .configured_stdin()
        .expect("rejected options retain the input file");
    let mut bytes = Vec::new();
    returned_input.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"async-preserve-on-manager-busy-close");
    release_tx.send(()).unwrap();
    busy_operation.join().unwrap();
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_async_standard_wait_returns_closed_after_manager_busy() {
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

    let (_, executor) = new_manager_busy_executor();
    let (release_tx, busy_operation) = hold_executor_manager_group(&executor);
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
    let failure = match waiting.as_mut().poll(&mut context) {
        Poll::Ready(Err(failure)) => failure,
        Poll::Ready(Ok(_)) => panic!("closed executor must reject manager Busy waiter"),
        Poll::Pending => panic!("shutdown must finish registered manager Busy wait"),
    };
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    release_tx.send(()).unwrap();
    busy_operation.join().unwrap();
}

#[cfg(all(feature = "async", unix, panic = "unwind"))]
#[test]
fn test_executor_shutdown_with_panicking_capacity_waker_still_closes_manager_group() {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    struct PanicWake;
    impl Wake for PanicWake {
        fn wake(self: Arc<Self>) {
            panic!("controlled capacity waiter panic");
        }
        fn wake_by_ref(self: &Arc<Self>) {
            panic!("controlled capacity waiter panic");
        }
    }

    let (_, executor) = new_manager_busy_executor();
    let (release_tx, busy_operation) = hold_executor_manager_group(&executor);
    let mut waiting = Box::pin(executor.submit_wait(
        CommandRunner::without_timeout(),
        Command::new("sh").arg("-c").arg("printf should-not-run"),
        Default::default(),
    ));
    let panic_waker = Waker::from(Arc::new(PanicWake));
    let mut panic_context = Context::from_waker(&panic_waker);
    assert!(matches!(waiting.as_mut().poll(&mut panic_context), Poll::Pending));
    assert_eq!(executor.scheduler_snapshot().waiters, 1);

    let shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    assert!(matches!(
        shutdown.wait_for(Duration::ZERO),
        Ok(qubit_operation::ShutdownWait::Pending(_))
    ));

    let failure = match waiting.as_mut().poll(&mut panic_context) {
        Poll::Ready(Err(failure)) => failure,
        Poll::Ready(Ok(_)) => panic!("closed executor must reject the waiter"),
        Poll::Pending => panic!("shutdown must complete despite a panicking waker"),
    };
    assert!(matches!(
        failure.reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    release_tx.send(()).unwrap();
    busy_operation.join().unwrap();
    assert!(matches!(
        shutdown.wait_for(Duration::from_secs(3)),
        Ok(qubit_operation::ShutdownWait::Completed(_))
    ));
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_scoped_async_waits_return_closed_after_manager_busy() {
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

    for resource in [false, true] {
        let (manager, executor) = new_manager_busy_executor();
        let (release_tx, busy_operation) = hold_executor_manager_group(&executor);
        let waiting_executor = executor.clone();
        let waiting_manager = manager.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let submitter = std::thread::spawn(move || {
            let _ = block_on(
                waiting_manager
                    .as_async()
                    .run(Default::default(), move |scope| async move {
                        let result: Result<(), (qubit_command::CommandSubmitFailureReason, Option<Vec<u8>>)> =
                            if resource {
                                let mut input = tempfile::tempfile().unwrap();
                                input.write_all(b"scoped-async-manager-busy").unwrap();
                                input.rewind().unwrap();
                                waiting_executor
                                    .submit_wait_resources_in(
                                        &scope,
                                        CommandRunner::without_timeout(),
                                        Command::new("sh").arg("-c").arg("cat"),
                                        CommandResourceRunOptions::new().stdin_file(input),
                                    )
                                    .await
                                    .map(|_| ())
                                    .map_err(|f| {
                                        let mut input = f.options.configured_stdin().unwrap();
                                        let mut bytes = Vec::new();
                                        input.read_to_end(&mut bytes).unwrap();
                                        (f.reason, Some(bytes))
                                    })
                            } else {
                                waiting_executor
                                    .submit_wait_in(
                                        &scope,
                                        CommandRunner::without_timeout(),
                                        Command::new("sh").arg("-c").arg("printf should-not-run"),
                                        Default::default(),
                                    )
                                    .await
                                    .map(|_| ())
                                    .map_err(|f| (f.reason, None))
                            };
                        tx.send(result).unwrap();
                        Ok::<_, ()>(())
                    }),
            );
        });
        wait_for_manager_busy_waiter(&executor);
        let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
        let (reason, bytes) = match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            Err(failure) => failure,
            Ok(()) => panic!("closed scoped executor must reject manager Busy waiter"),
        };
        submitter.join().unwrap();
        assert!(matches!(
            reason,
            qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
        ));
        if resource {
            assert_eq!(bytes.unwrap(), b"scoped-async-manager-busy");
        }
        release_tx.send(()).unwrap();
        busy_operation.join().unwrap();
    }
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_scoped_async_resource_wait_returns_closed_after_shutdown() {
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

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(
        manager.clone(),
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let mut active = occupying_command(&executor);
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let waiting_manager = manager.clone();
    let submitter = std::thread::spawn(move || {
        let outcome = block_on(
            waiting_manager
                .as_async()
                .run(Default::default(), move |scope| async move {
                    let mut input = tempfile::tempfile().unwrap();
                    input.write_all(b"scoped-async-preserve-on-close").unwrap();
                    input.rewind().unwrap();
                    let failure = match waiting_executor
                        .submit_wait_resources_in(
                            &scope,
                            CommandRunner::without_timeout(),
                            Command::new("sh").arg("-c").arg("cat"),
                            CommandResourceRunOptions::new().stdin_file(input),
                        )
                        .await
                    {
                        Err(failure) => failure,
                        Ok(_) => panic!("closed scoped async executor must reject the waiter"),
                    };
                    let mut input = failure
                        .options
                        .configured_stdin()
                        .expect("rejected options retain the input file");
                    let mut bytes = Vec::new();
                    input.read_to_end(&mut bytes).unwrap();
                    tx.send((failure.reason, bytes)).unwrap();
                    Ok::<_, ()>(())
                }),
        )
        .unwrap();
        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "scoped async waiter did not register"
        );
        std::thread::yield_now();
    }
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let (reason, bytes) = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("closed scoped async wait should return");
    submitter.join().unwrap();
    assert!(matches!(
        reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    assert_eq!(bytes, b"scoped-async-preserve-on-close");
    let _ = active.wait_outcome();
}

#[cfg(all(feature = "async", unix))]
#[test]
fn test_scoped_async_standard_wait_returns_closed_after_shutdown() {
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
    let executor = CommandExecutor::new(
        manager.clone(),
        CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            max_waiters: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let mut active = occupying_command(&executor);
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting_executor = executor.clone();
    let waiting_manager = manager.clone();
    let submitter = std::thread::spawn(move || {
        let _ = block_on(
            waiting_manager
                .as_async()
                .run(Default::default(), move |scope| async move {
                    let failure = match waiting_executor
                        .submit_wait_in(
                            &scope,
                            CommandRunner::without_timeout(),
                            Command::new("sh").arg("-c").arg("printf should-not-run"),
                            Default::default(),
                        )
                        .await
                    {
                        Err(failure) => failure,
                        Ok(_) => panic!("closed scoped async executor must reject the waiter"),
                    };
                    tx.send(failure.reason).unwrap();
                    Ok::<_, ()>(())
                }),
        );
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while executor.scheduler_snapshot().waiters == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "scoped async waiter did not register"
        );
        std::thread::yield_now();
    }
    let _shutdown = executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);
    let reason = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("closed scoped async wait should return");
    submitter.join().unwrap();
    assert!(matches!(
        reason,
        qubit_command::CommandSubmitFailureReason::Capacity(qubit_command::execution::CapacityFailure::Closed)
    ));
    let _ = active.wait_outcome();
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
