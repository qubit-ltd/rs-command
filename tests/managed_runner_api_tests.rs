// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
use std::io::Read;
use std::time::Duration;

use qubit_command::Command;
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
    let resources = CommandResourceRunOptions::new()
        .stdout_sink(sink.try_clone().unwrap(), FileSinkPolicy::Preserve);
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

#[cfg(all(feature = "async", unix))]
mod async_tests {
    use super::*;
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
        let options = CommandResourceRunOptions::new()
            .stdout_sink(sink.try_clone().unwrap(), FileSinkPolicy::Preserve);
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
                let resources = block_on(
                    runner().run_async_resources_in(
                        &scope,
                        &executor,
                        Command::new("sh")
                            .arg("-c")
                            .arg("printf async-resource-scoped"),
                        CommandResourceRunOptions::new(),
                    ),
                )?;
                assert_eq!(resources.stdout(), b"async-resource-scoped");
                Ok::<_, qubit_command::CommandError>(ordinary)
            })
            .unwrap();
        match result {
            OperationOutcome::Succeeded(output) => assert_eq!(output.stdout(), b"async-scoped"),
            other => panic!("managed child operation failed: {other:?}"),
        }
    }
}
