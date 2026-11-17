// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io::Read;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunner;
use qubit_command::FileSinkPolicy;
use qubit_command::ManagedCommandRunnerApi;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;

fn executor(manager: &OperationManager) -> CommandExecutor {
    CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).unwrap()
}

fn runner() -> CommandRunner {
    CommandRunner::new(Duration::from_secs(5))
}

#[cfg(unix)]
#[test]
fn test_sync_managed_runner_methods_capture_output_and_write_owned_sinks() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = executor(&manager);
    let ordinary = runner()
        .run_managed(
            &executor,
            Command::new("sh").arg("-c").arg("printf ordinary"),
            Default::default(),
        )
        .unwrap();
    assert_eq!(ordinary.stdout(), b"ordinary");

    let mut sink = tempfile::tempfile().unwrap();
    let resources = CommandResourceRunOptions::new().stdout_sink(sink.try_clone().unwrap(), FileSinkPolicy::Preserve);
    let output = runner()
        .run_managed_resources(
            &executor,
            Command::new("sh").arg("-c").arg("printf resource"),
            resources,
        )
        .unwrap();
    assert_eq!(output.stdout(), b"resource");
    use std::io::Seek;
    use std::io::SeekFrom;
    sink.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    sink.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"resource");
}

#[cfg(unix)]
#[test]
fn test_scoped_managed_runner_methods_return_process_results() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = executor(&manager);
    let result = manager
        .run(OperationOptions::default(), move |scope| {
            let ordinary = runner().run_managed_in(
                &scope,
                &executor,
                Command::new("sh").arg("-c").arg("printf scoped"),
                Default::default(),
            )?;
            let options = CommandResourceRunOptions::new();
            let resources = runner().run_managed_resources_in(
                &scope,
                &executor,
                Command::new("sh").arg("-c").arg("printf resource-scoped"),
                options,
            )?;
            assert_eq!(resources.stdout(), b"resource-scoped");
            Ok::<_, qubit_command::CommandError>(ordinary)
        })
        .unwrap();
    match result {
        OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"scoped"),
        other => panic!("managed child operation failed: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn test_sync_managed_runner_preserves_failed_command_output() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = executor(&manager);
    let error = runner()
        .run_managed(
            &executor,
            Command::new("sh")
                .arg("-c")
                .arg("printf managed-out; printf managed-err >&2; exit 7"),
            Default::default(),
        )
        .expect_err("managed command should report its unexpected exit");

    assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
    assert_eq!(error.exit_code(), Some(7));
    assert_eq!(error.output().unwrap().stdout(), b"managed-out");
    assert_eq!(error.output().unwrap().stderr(), b"managed-err");
}

#[cfg(all(feature = "async", unix))]
mod async_tests {
    use std::future::Future;
    use std::io::Read;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandErrorKind;
    use qubit_command::CommandResourceRunOptions;
    use qubit_command::FileSinkPolicy;
    use qubit_command::ManagedCommandRunnerApi;
    use qubit_command::session::CommandSessionApi;
    use qubit_operation::GroupOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::executor;
    use super::runner;

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

    #[test]
    fn test_async_managed_runner_methods_collect_output_and_settle() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = executor(&manager);
        let output = block_on(runner().run_async(
            &executor,
            Command::new("sh").arg("-c").arg("printf asynchronous"),
            Default::default(),
        ))
        .unwrap();
        assert_eq!(output.stdout(), b"asynchronous");

        let mut sink = tempfile::tempfile().unwrap();
        let options = CommandResourceRunOptions::new().stdout_sink(sink.try_clone().unwrap(), FileSinkPolicy::Preserve);
        let output = block_on(runner().run_async_resources(
            &executor,
            Command::new("sh").arg("-c").arg("printf async-resource"),
            options,
        ))
        .unwrap();
        assert_eq!(output.stdout(), b"async-resource");
        use std::io::Seek;
        use std::io::SeekFrom;
        sink.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        sink.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"async-resource");
    }

    #[test]
    fn test_async_managed_runner_preserves_failed_command_output() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = executor(&manager);
        let error = block_on(
            runner().run_async(
                &executor,
                Command::new("sh")
                    .arg("-c")
                    .arg("printf async-managed-out; printf async-managed-err >&2; exit 9"),
                Default::default(),
            ),
        )
        .expect_err("async managed command should report its unexpected exit");

        assert_eq!(error.kind(), CommandErrorKind::UnexpectedExit);
        assert_eq!(error.exit_code(), Some(9));
        assert_eq!(error.output().unwrap().stdout(), b"async-managed-out");
        assert_eq!(error.output().unwrap().stderr(), b"async-managed-err");
    }

    #[test]
    fn test_scoped_async_managed_runner_methods_collect_child_results() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let operation_manager = manager.clone();
        let result = manager
            .run(OperationOptions::default(), move |scope| {
                let executor = executor(&operation_manager);
                let ordinary = block_on(runner().run_async_in(
                    &scope,
                    &executor,
                    Command::new("sh").arg("-c").arg("printf async-scoped"),
                    Default::default(),
                ))?;
                let resources = block_on(runner().run_async_resources_in(
                    &scope,
                    &executor,
                    Command::new("sh").arg("-c").arg("printf async-resource-scoped"),
                    CommandResourceRunOptions::new(),
                ))?;
                assert_eq!(resources.stdout(), b"async-resource-scoped");
                Ok::<_, qubit_command::CommandError>(ordinary)
            })
            .unwrap();
        match result {
            OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"async-scoped"),
            other => panic!("managed child operation failed: {other:?}"),
        }
    }

    #[test]
    fn test_session_runner_methods_start_root_and_scoped_sessions() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = executor(&manager);
        for result in [
            block_on(runner().start_session(&executor, Command::new("rustc").arg("--version"), Default::default())),
            block_on(runner().start_session_resources(
                &executor,
                Command::new("rustc").arg("--version"),
                CommandResourceRunOptions::new(),
            )),
        ] {
            let session = result.unwrap();
            assert!(session.operation().is_some());
            drop(session);
        }

        let group = manager.group(GroupOptions { capacity: 4 }).unwrap();
        let _completion = group
            .run_and_settle(OperationOptions::default(), move |scope| {
                let session = block_on(runner().start_session_in(
                    &scope,
                    &executor,
                    Command::new("rustc").arg("--version"),
                    Default::default(),
                ))?;
                assert!(session.operation().is_some());
                drop(session);
                let session = block_on(runner().start_session_resources_in(
                    &scope,
                    &executor,
                    Command::new("rustc").arg("--version"),
                    CommandResourceRunOptions::new(),
                ))?;
                assert!(session.operation().is_some());
                drop(session);
                Ok::<_, qubit_command::CommandError>(())
            })
            .unwrap();
    }
}
