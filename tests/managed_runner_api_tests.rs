// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use qubit_command::Command;
#[cfg(unix)]
use qubit_command::CommandErrorKind;
#[cfg(unix)]
use qubit_command::CommandErrorReason;
#[cfg(unix)]
use qubit_command::CommandExecutionConfig;
#[cfg(unix)]
use qubit_command::CommandExecutor;
#[cfg(unix)]
use qubit_command::CommandResourceRunOptions;
#[cfg(unix)]
use qubit_command::CommandRunner;
#[cfg(unix)]
use qubit_command::FileSinkPolicy;
#[cfg(unix)]
use qubit_command::ManagedCommandRunnerApi;
#[cfg(unix)]
use qubit_operation::ManagerOptions;
#[cfg(unix)]
use qubit_operation::OperationManager;
#[cfg(unix)]
use qubit_operation::OperationOptions;
#[cfg(unix)]
use qubit_operation::OperationOutcome;

#[cfg(unix)]
fn executor(manager: &OperationManager) -> CommandExecutor {
    CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).unwrap()
}

#[cfg(unix)]
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

#[cfg(unix)]
#[test]
fn test_scoped_managed_runner_maps_manager_mismatch_for_ordinary_and_resource_calls() {
    let scope_manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let other_manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let wrong_executor = executor(&other_manager);
    let result = scope_manager
        .run(OperationOptions::default(), move |scope| {
            let ordinary = runner().run_managed_in(
                &scope,
                &wrong_executor,
                Command::new("sh").arg("-c").arg("printf must-not-run"),
                Default::default(),
            );
            let resources = runner().run_managed_resources_in(
                &scope,
                &wrong_executor,
                Command::new("sh").arg("-c").arg("printf must-not-run"),
                CommandResourceRunOptions::new(),
            );
            assert_manager_mismatch(ordinary.unwrap_err());
            assert_manager_mismatch(resources.unwrap_err());
            Ok::<_, qubit_command::CommandError>(())
        })
        .unwrap();
    assert!(matches!(result, OperationOutcome::Succeeded(())));
}

#[cfg(unix)]
fn assert_manager_mismatch(error: qubit_command::CommandError) {
    assert_eq!(error.kind(), CommandErrorKind::ManagedOperationFailed);
    assert!(matches!(
        error.reason(),
        CommandErrorReason::ManagedOperationFailed { diagnostic }
            if diagnostic == "submission rejected: GroupBinding(WrongManager)"
    ));
    assert!(error.output().is_none());
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
    fn test_scoped_async_managed_runner_maps_manager_mismatch_for_ordinary_and_resource_calls() {
        let scope_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let other_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let wrong_executor = executor(&other_manager);
        let result = scope_manager
            .run(OperationOptions::default(), move |scope| {
                let ordinary = block_on(runner().run_async_in(
                    &scope,
                    &wrong_executor,
                    Command::new("sh").arg("-c").arg("printf must-not-run"),
                    Default::default(),
                ));
                let resources = block_on(runner().run_async_resources_in(
                    &scope,
                    &wrong_executor,
                    Command::new("sh").arg("-c").arg("printf must-not-run"),
                    CommandResourceRunOptions::new(),
                ));
                super::assert_manager_mismatch(ordinary.unwrap_err());
                super::assert_manager_mismatch(resources.unwrap_err());
                Ok::<_, qubit_command::CommandError>(())
            })
            .unwrap();
        assert!(matches!(result, OperationOutcome::Succeeded(())));
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

    #[test]
    fn test_session_runner_methods_return_submission_rejections() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let closed_executor = executor(&manager);
        let _shutdown = closed_executor.request_shutdown(qubit_operation::ShutdownMode::Cancel);

        for (result, diagnostic_prefix) in [
            (
                block_on(runner().start_session(
                    &closed_executor,
                    Command::new("__must_not_spawn_after_session_executor_shutdown__"),
                    Default::default(),
                )),
                "managed session admission failed:",
            ),
            (
                block_on(runner().start_session_resources(
                    &closed_executor,
                    Command::new("__must_not_spawn_after_session_executor_shutdown__"),
                    CommandResourceRunOptions::new(),
                )),
                "managed resource session admission failed:",
            ),
        ] {
            let error = match result {
                Ok(_) => panic!("closed executor should reject a session request"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), CommandErrorKind::ManagedOperationFailed);
            assert!(matches!(
                error.reason(),
                qubit_command::CommandErrorReason::ManagedOperationFailed { diagnostic }
                    if diagnostic.starts_with(diagnostic_prefix)
            ));
            assert!(error.output().is_none());
        }

        let scope_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let other_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let wrong_executor = executor(&other_manager);
        let result = scope_manager
            .run(OperationOptions::default(), move |scope| {
                let ordinary = block_on(runner().start_session_in(
                    &scope,
                    &wrong_executor,
                    Command::new("__must_not_spawn_for_foreign_session_scope__"),
                    Default::default(),
                ));
                let resources = block_on(runner().start_session_resources_in(
                    &scope,
                    &wrong_executor,
                    Command::new("__must_not_spawn_for_foreign_session_scope__"),
                    CommandResourceRunOptions::new(),
                ));

                for result in [ordinary, resources] {
                    let error = match result {
                        Ok(_) => panic!("session scope must reject a foreign executor"),
                        Err(error) => error,
                    };
                    assert_eq!(error.kind(), CommandErrorKind::ManagedOperationFailed);
                    assert!(matches!(
                        error.reason(),
                        qubit_command::CommandErrorReason::ManagedOperationFailed { diagnostic }
                            if diagnostic.starts_with("group binding failed:")
                    ));
                    assert!(error.output().is_none());
                }
                Ok::<_, qubit_command::CommandError>(())
            })
            .unwrap();
        assert!(matches!(result, OperationOutcome::Succeeded(())));
    }
}
