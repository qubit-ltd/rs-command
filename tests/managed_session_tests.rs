// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(feature = "async")]

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::thread;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandErrorReason;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
use qubit_command::ManagedCommandRunnerApi;
use qubit_command::session::CommandSessionApi;
use qubit_io::AsyncClose;
use qubit_io::AsyncInput;
use qubit_io::AsyncOutput;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;

struct ThreadWake(thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark()
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark()
    }
}
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        thread::park_timeout(Duration::from_secs(2));
    }
}

async fn read_all(reader: &mut (impl AsyncInput<Item = u8> + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 128];
    loop {
        let count = reader.read_async(&mut chunk).await?;
        if count == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

#[cfg(unix)]
#[test]
fn test_managed_session_returns_endpoints_and_streams_both_pipes() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let runner = CommandRunner::new(Duration::from_secs(5));
    let mut session = block_on(runner.start_session(
        &executor,
        Command::new("sh").arg("-c").arg("printf out; printf err >&2"),
        Default::default(),
    ))
    .unwrap();
    let mut stdin = session.take_stdin().unwrap();
    block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).unwrap();
    drop(stdin);
    let mut stdout = session.take_stdout().unwrap();
    let mut stderr = session.take_stderr().unwrap();
    let output = block_on(async move {
        let out = async {
            let mut all = Vec::new();
            let mut buf = [0; 128];
            loop {
                let n = poll_fn(|cx| Pin::new(&mut stdout).poll_read(cx, &mut buf))
                    .await
                    .unwrap();
                if n == 0 {
                    break;
                }
                all.extend_from_slice(&buf[..n]);
            }
            all
        };
        let err = async {
            let mut all = Vec::new();
            let mut buf = [0; 128];
            loop {
                let n = poll_fn(|cx| Pin::new(&mut stderr).poll_read(cx, &mut buf))
                    .await
                    .unwrap();
                if n == 0 {
                    break;
                }
                all.extend_from_slice(&buf[..n]);
            }
            all
        };
        futures_join(out, err).await
    });
    assert_eq!(output.0, b"out");
    assert_eq!(output.1, b"err");
    let mut operation = session.take_operation().unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    operation.wait_settlement().unwrap();
}

#[cfg(unix)]
#[test]
fn test_managed_resource_session_returns_streams_and_operation() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut session = block_on(CommandRunner::without_timeout().start_session_resources(
        &executor,
        Command::new("sh").arg("-c").arg("cat; printf resource-session >&2"),
        CommandResourceRunOptions::new(),
    ))
    .unwrap();
    let mut stdin = session.take_stdin().unwrap();
    block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_write(cx, b"resource input"))).unwrap();
    block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).unwrap();
    drop(stdin);

    let mut stdout = session.take_stdout().unwrap();
    let mut stderr = session.take_stderr().unwrap();
    assert_eq!(block_on(read_all(&mut stdout)).unwrap(), b"resource input");
    assert_eq!(block_on(read_all(&mut stderr)).unwrap(), b"resource-session");
    let mut operation = session.take_operation().unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    operation.wait_settlement().unwrap();
}

#[cfg(unix)]
#[test]
fn test_resource_session_owned_stdin_conflict_preserves_sink_before_start() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let input_path = directory.path().join("input");
    let output_path = directory.path().join("output");
    let started_path = directory.path().join("started");
    std::fs::write(&input_path, b"owned input").unwrap();
    std::fs::write(&output_path, b"preserve sink").unwrap();
    let input = std::fs::File::open(&input_path).unwrap();
    let output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&output_path)
        .unwrap();

    let mut ordinary = block_on(
        CommandRunner::without_timeout().start_session(
            &executor,
            Command::new("sh")
                .arg("-c")
                .arg("exit 0")
                .stdin_file(input_path.clone()),
            CommandRunOptions::new(),
        ),
    )
    .expect("ordinary session submission should be accepted");
    let mut ordinary_operation = ordinary.take_operation().unwrap();
    let OperationOutcome::Failed(ordinary_error) = ordinary_operation.wait_outcome().unwrap() else {
        panic!("ordinary session stdin files must be rejected before startup");
    };
    assert_eq!(ordinary_error.kind(), qubit_command::CommandErrorKind::WriteInputFailed);

    let mut session = block_on(
        CommandRunner::without_timeout().start_session_resources(
            &executor,
            Command::new("sh")
                .arg("-c")
                .arg("touch \"$1\"")
                .arg("resource-session-conflict")
                .arg(started_path.to_str().unwrap()),
            CommandResourceRunOptions::new()
                .stdin_file(input)
                .stdout_sink(output, qubit_command::FileSinkPolicy::Truncate),
        ),
    )
    .expect("resource session submission should be accepted");
    let mut operation = session.take_operation().unwrap();
    let OperationOutcome::Failed(error) = operation.wait_outcome().unwrap() else {
        panic!("resource session stdin files must be rejected before startup");
    };
    assert!(matches!(
        error.reason(),
        qubit_command::CommandErrorReason::FilePreparationFailed {
            stage: qubit_command::FilePreparationStage::Validation,
            direction: Some(qubit_command::FilePreparationDirection::Stdin),
            ..
        }
    ));
    assert_eq!(std::fs::read(&output_path).unwrap(), b"preserve sink");
    assert!(
        !started_path.exists(),
        "the resource session must be rejected before spawn"
    );
}

#[test]
fn test_resource_session_rejects_path_stdin_before_registration_or_start() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let input_path = directory.path().join("input");
    let output_path = directory.path().join("output");
    std::fs::write(&input_path, b"path input").unwrap();
    std::fs::write(&output_path, b"preserve sink").unwrap();
    let output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&output_path)
        .unwrap();
    let mut session = block_on(CommandRunner::without_timeout().start_session_resources(
        &executor,
        Command::new("__qubit_command_must_not_start_resource_path_stdin_session__").stdin_file(&input_path),
        CommandResourceRunOptions::new().stdout_sink(output, qubit_command::FileSinkPolicy::Truncate),
    ))
    .unwrap();
    let mut operation = session.take_operation().unwrap();
    let OperationOutcome::Failed(error) = operation.wait_outcome().unwrap() else {
        panic!("resource sessions must reject path stdin before startup");
    };
    assert!(matches!(
        error.reason(),
        qubit_command::CommandErrorReason::FilePreparationFailed {
            stage: qubit_command::FilePreparationStage::Validation,
            direction: Some(qubit_command::FilePreparationDirection::Stdin),
            source,
        } if source.kind() == std::io::ErrorKind::InvalidInput
    ));
    assert_eq!(std::fs::read(&output_path).unwrap(), b"preserve sink");
    let settlement = operation.wait_settlement().unwrap();
    assert_eq!(settlement.status, qubit_operation::SettlementStatus::RecoveryRequired);
    assert_eq!(settlement.effects_total, 0);
}

#[cfg(unix)]
#[test]
fn test_resource_session_allows_command_byte_prefix_and_owned_sink() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let output = tempfile::NamedTempFile::new().unwrap();
    let output_path = output.path().to_path_buf();
    std::fs::write(&output_path, b"old sink").unwrap();
    let sink = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&output_path)
        .unwrap();
    let mut session = block_on(CommandRunner::without_timeout().start_session_resources(
        &executor,
        Command::new("sh").arg("-c").arg("cat").stdin_bytes(b"prefix-"),
        CommandResourceRunOptions::new().stdout_sink(sink, qubit_command::FileSinkPolicy::Truncate),
    ))
    .unwrap();
    let mut input = session.take_stdin().unwrap();
    block_on(poll_fn(|cx| Pin::new(&mut input).poll_write(cx, b"session"))).unwrap();
    block_on(poll_fn(|cx| Pin::new(&mut input).poll_close(cx))).unwrap();
    drop(input);
    let mut operation = session.take_operation().unwrap();
    let OperationOutcome::Succeeded(output) = operation.wait_outcome().unwrap() else {
        panic!("session pipe should accept its command byte prefix");
    };
    assert_eq!(output.stdout(), b"prefix-session");
    assert_eq!(std::fs::read(output_path).unwrap(), b"prefix-session");
    assert!(operation.wait_settlement().unwrap().all_released());
}

#[cfg(unix)]
#[test]
fn test_scoped_resource_session_owned_stdin_conflict_preserves_sink_before_start() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let child_manager = manager.clone();
    let directory = tempfile::tempdir().unwrap();
    let input_path = directory.path().join("input");
    let output_path = directory.path().join("output");
    let started_path = directory.path().join("started");
    std::fs::write(&input_path, b"owned input").unwrap();
    std::fs::write(&output_path, b"preserve scoped sink").unwrap();
    let output_path_for_scope = output_path.clone();
    let started_path_for_scope = started_path.clone();
    let _ = manager
        .run(OperationOptions::default(), move |scope| {
            let executor = CommandExecutor::new(child_manager, CommandExecutionConfig::default()).unwrap();
            let input = std::fs::File::open(&input_path).unwrap();
            let output = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&output_path_for_scope)
                .unwrap();
            let mut session = block_on(
                CommandRunner::without_timeout().start_session_resources_in(
                    &scope,
                    &executor,
                    Command::new("sh")
                        .arg("-c")
                        .arg("touch \"$1\"")
                        .arg("scoped-resource-session-conflict")
                        .arg(started_path_for_scope.to_str().unwrap()),
                    CommandResourceRunOptions::new()
                        .stdin_file(input)
                        .stdout_sink(output, qubit_command::FileSinkPolicy::Truncate),
                ),
            )
            .expect("scoped resource session should be accepted");
            let mut operation = session.take_operation().unwrap();
            let OperationOutcome::Failed(error) = operation.wait_outcome().unwrap() else {
                panic!("scoped resource session stdin files must be rejected");
            };
            assert!(matches!(
                error.reason(),
                qubit_command::CommandErrorReason::FilePreparationFailed {
                    stage: qubit_command::FilePreparationStage::Validation,
                    direction: Some(qubit_command::FilePreparationDirection::Stdin),
                    ..
                }
            ));
            assert_eq!(std::fs::read(&output_path_for_scope).unwrap(), b"preserve scoped sink");
            assert!(!started_path_for_scope.exists());
            Ok::<_, std::convert::Infallible>(())
        })
        .unwrap();
}

#[cfg(unix)]
#[test]
fn test_scoped_managed_resource_session_finishes_with_parent_scope() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let child_manager = manager.clone();
    let result = manager
        .run(OperationOptions::default(), move |scope| {
            let executor = CommandExecutor::new(child_manager, CommandExecutionConfig::default()).unwrap();
            let mut session = block_on(CommandRunner::without_timeout().start_session_resources_in(
                &scope,
                &executor,
                Command::new("sh").arg("-c").arg("cat"),
                CommandResourceRunOptions::new(),
            ))?;
            let mut stdin = session.take_stdin().unwrap();
            block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_write(cx, b"scoped resource")))
                .expect("session stdin should accept bytes");
            block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).expect("session stdin should close");
            drop(stdin);

            let mut stdout = session.take_stdout().unwrap();
            let mut stderr = session.take_stderr().unwrap();
            let output = block_on(read_all(&mut stdout)).expect("stdout should finish");
            assert!(
                block_on(read_all(&mut stderr))
                    .expect("stderr should finish")
                    .is_empty()
            );
            let mut operation = session.take_operation().unwrap();
            assert!(matches!(
                operation.wait_outcome().unwrap(),
                OperationOutcome::Succeeded(_)
            ));
            operation.wait_settlement().unwrap();
            Ok::<_, qubit_command::CommandError>(output)
        })
        .unwrap();
    match result {
        OperationOutcome::Succeeded(output) => assert_eq!(output, b"scoped resource"),
        other => panic!("scoped managed session failed: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn test_managed_session_input_flush_and_buffering_round_trip() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let mut session = block_on(CommandRunner::new(Duration::from_secs(5)).start_session(
        &executor,
        Command::new("sh").arg("-c").arg("cat"),
        Default::default(),
    ))
    .unwrap();
    let mut stdin = session.take_stdin().unwrap();
    assert!(stdin.is_buffered());
    assert_eq!(
        block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_write(cx, b"session input"))).unwrap(),
        13
    );
    block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_flush(cx))).unwrap();
    block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).unwrap();
    drop(stdin);

    let mut stdout = session.take_stdout().unwrap();
    let mut stderr = session.take_stderr().unwrap();
    assert!(stdout.is_buffered());
    assert!(stderr.is_buffered());
    assert_eq!(block_on(read_all(&mut stdout)).unwrap(), b"session input");
    assert!(block_on(read_all(&mut stderr)).unwrap().is_empty());
    let mut operation = session.take_operation().unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    operation.wait_settlement().unwrap();
}

#[cfg(unix)]
#[test]
fn test_managed_session_applies_runner_total_output_budget() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let runner = CommandRunner::without_timeout().max_total_output_bytes(8);
    let mut session = block_on(runner.start_session(
        &executor,
        Command::new("sh").arg("-c").arg("printf 12345678901234567890"),
        CommandRunOptions::new(),
    ))
    .unwrap();
    let mut stdin = session.take_stdin().unwrap();
    let mut stdout = session.take_stdout().unwrap();
    let mut stderr = session.take_stderr().unwrap();
    let mut operation = session.take_operation().unwrap();

    let (out, err) = thread::scope(|scope| {
        let out_reader = scope.spawn(|| block_on(read_all(&mut stdout)));
        let err_reader = scope.spawn(|| block_on(read_all(&mut stderr)));
        block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).unwrap();
        (out_reader.join().unwrap().unwrap(), err_reader.join().unwrap().unwrap())
    });
    assert!(!out.is_empty());
    assert!(err.is_empty());
    let outcome = operation.wait_outcome().unwrap();
    assert!(matches!(
        outcome,
        OperationOutcome::Failed(ref error)
            if matches!(error.reason(), CommandErrorReason::OutputLimitExceeded { limit: 8, observed } if *observed > 8)
    ));
    drop((stdin, stdout, stderr));
    operation.wait_settlement().unwrap();
}

#[cfg(unix)]
#[test]
fn test_managed_session_preserves_both_slow_streams_within_budget() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let payload_size = 128 * 1024;
    let command =
        format!("head -c {payload_size} /dev/zero | tr '\\000' x; head -c {payload_size} /dev/zero | tr '\\000' y >&2");
    let runner = CommandRunner::without_timeout().max_total_output_bytes(1024 * 1024);
    let mut session = block_on(runner.start_session(
        &executor,
        Command::new("sh").arg("-c").arg(&command),
        CommandRunOptions::new(),
    ))
    .unwrap();
    let mut stdin = session.take_stdin().unwrap();
    let mut stdout = session.take_stdout().unwrap();
    let mut stderr = session.take_stderr().unwrap();
    let mut operation = session.take_operation().unwrap();

    thread::sleep(Duration::from_millis(50));
    let (out, err) = thread::scope(|scope| {
        let out_reader = scope.spawn(|| block_on(read_all(&mut stdout)));
        let err_reader = scope.spawn(|| block_on(read_all(&mut stderr)));
        block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).unwrap();
        (out_reader.join().unwrap().unwrap(), err_reader.join().unwrap().unwrap())
    });

    assert_eq!(out, vec![b'x'; payload_size]);
    assert_eq!(err, vec![b'y'; payload_size]);
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    drop((stdin, stdout, stderr));
    operation.wait_settlement().unwrap();
}

#[cfg(unix)]
#[test]
fn test_managed_session_zero_budget_accepts_empty_output() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let runner = CommandRunner::without_timeout().max_total_output_bytes(0);
    let mut session = block_on(runner.start_session(
        &executor,
        Command::new("sh").arg("-c").arg(":"),
        CommandRunOptions::new(),
    ))
    .unwrap();
    let mut stdin = session.take_stdin().unwrap();
    let mut stdout = session.take_stdout().unwrap();
    let mut stderr = session.take_stderr().unwrap();
    let mut operation = session.take_operation().unwrap();

    let (out, err) = thread::scope(|scope| {
        let out_reader = scope.spawn(|| block_on(read_all(&mut stdout)));
        let err_reader = scope.spawn(|| block_on(read_all(&mut stderr)));
        block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).unwrap();
        (out_reader.join().unwrap().unwrap(), err_reader.join().unwrap().unwrap())
    });

    assert!(out.is_empty());
    assert!(err.is_empty());
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    drop((stdin, stdout, stderr));
    operation.wait_settlement().unwrap();
}

#[cfg(unix)]
#[test]
fn test_dropping_session_requests_stop_and_preserves_observable_settlement() {
    use qubit_operation::SettlementWait;
    use qubit_operation::ShutdownMode;
    use qubit_operation::StopReason;
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let runner = CommandRunner::without_timeout();
    let session = block_on(runner.start_session(
        &executor,
        Command::new("sh").arg("-c").arg("sleep 10"),
        Default::default(),
    ))
    .unwrap();
    let observer = session.operation().unwrap().observer();

    drop(session);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let stop = loop {
        if let Some(stop) = observer.snapshot().stop {
            break stop;
        }
        assert!(std::time::Instant::now() < deadline, "session drop must request stop");
        thread::yield_now();
    };
    assert_eq!(stop.reason, StopReason::UserCancelled);
    let shutdown = executor.request_shutdown(ShutdownMode::Drain);
    let _ = shutdown.wait_for(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        observer.wait_settlement_for(Duration::from_secs(5)).unwrap(),
        SettlementWait::Ready(_)
    ));
}

async fn futures_join<A: std::future::Future, B: std::future::Future>(a: A, b: B) -> (A::Output, B::Output) {
    let mut a = Box::pin(a);
    let mut b = Box::pin(b);
    let mut ao = None;
    let mut bo = None;
    poll_fn(|cx| {
        if ao.is_none()
            && let Poll::Ready(x) = a.as_mut().poll(cx)
        {
            ao = Some(x)
        }
        if bo.is_none()
            && let Poll::Ready(x) = b.as_mut().poll(cx)
        {
            bo = Some(x)
        }
        if ao.is_some() && bo.is_some() {
            Poll::Ready((ao.take().unwrap(), bo.take().unwrap()))
        } else {
            Poll::Pending
        }
    })
    .await
}
