// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::future::Future;
use std::hint::black_box;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
use qubit_command::CommandSessionApi;
use qubit_command::ManagedCommandRunnerApi;
use qubit_io::AsyncClose;
use qubit_io::AsyncInput;
use qubit_operation::ManagerLimits;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;
use qubit_operation::ScopeLimits;
use qubit_operation::StopReason;
use qubit_operation::ThreadPoolExecutorOptions;

static REPORTED_DISTRIBUTIONS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

fn report_distribution(label: String, values: &mut [Duration]) {
    if std::env::var_os("RS_COMMAND_BENCH_REPORT").is_none() {
        return;
    }
    let reported = REPORTED_DISTRIBUTIONS.get_or_init(Default::default);
    let mut reported = reported.lock().expect("distribution report lock");
    if !reported.insert(label.clone()) {
        return;
    }
    drop(reported);
    if values.is_empty() {
        eprintln!("{label}: no samples");
        return;
    }
    values.sort_unstable();
    let percentile = |p: usize| values[(values.len() - 1) * p / 100].as_secs_f64() * 1000.0;
    eprintln!(
        "{label}: n={} p50_ms={:.3} p95_ms={:.3}",
        values.len(),
        percentile(50),
        percentile(95)
    );
}

fn fixture() -> Command {
    if cfg!(windows) {
        Command::new("cmd").args(&["/C", "echo managed-output"])
    } else {
        Command::new("sh").args(&["-c", "printf managed-output"])
    }
}

fn sleeping_fixture() -> Command {
    if cfg!(windows) {
        Command::new("powershell").args(&["-NoProfile", "-Command", "Start-Sleep -Milliseconds 30"])
    } else {
        Command::new("sh").args(&["-c", "sleep 0.03"])
    }
}

fn executor(max_active: usize, max_queued: usize) -> CommandExecutor {
    let manager = OperationManager::new(ManagerOptions {
        limits: ManagerLimits {
            active_operations: 128,
            queued_operations: 128,
            ..ManagerLimits::default()
        },
        scope_limits: ScopeLimits {
            child_operations: 128,
            ..ScopeLimits::default()
        },
        sync_executor: ThreadPoolExecutorOptions {
            worker_count: 8,
            queue_capacity: 128,
        },
        ..ManagerOptions::default()
    })
    .expect("operation manager");
    CommandExecutor::new(
        manager,
        CommandExecutionConfig {
            max_active,
            max_queued,
            ..CommandExecutionConfig::default()
        },
    )
    .expect("command executor")
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWake(thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => thread::park_timeout(Duration::from_secs(1)),
        }
    }
}

async fn read_all(reader: &mut (impl AsyncInput<Item = u8> + Unpin)) -> io::Result<Vec<u8>> {
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

fn stream_once(executor: &CommandExecutor) {
    let runner = CommandRunner::without_timeout();
    let mut session = block_on(runner.start_session(executor, fixture(), CommandRunOptions::new()))
        .expect("start managed session");
    let mut stdin = session.take_stdin().expect("stdin endpoint");
    let mut stdout = session.take_stdout().expect("stdout endpoint");
    let mut stderr = session.take_stderr().expect("stderr endpoint");
    let mut operation = session.take_operation().expect("operation handle");

    let (out, err) = thread::scope(|scope| {
        let out_reader = scope.spawn(|| block_on(read_all(&mut stdout)));
        let err_reader = scope.spawn(|| block_on(read_all(&mut stderr)));
        block_on(std::future::poll_fn(|cx| {
            Pin::new(&mut stdin).poll_close(cx)
        }))
        .expect("close session stdin");
        (
            out_reader.join().expect("stdout reader").expect("stdout"),
            err_reader.join().expect("stderr reader").expect("stderr"),
        )
    });
    assert!(!out.is_empty());
    black_box(err);
    assert!(matches!(
        operation.wait_outcome().expect("operation outcome"),
        OperationOutcome::Succeeded(_)
    ));
    drop((stdin, stdout, stderr));
    operation.wait_settlement().expect("operation settlement");
}

fn batch_once(executor: &CommandExecutor, count: usize) {
    let runner = CommandRunner::without_timeout();
    let mut operations = (0..count)
        .map(|_| {
            let submitted_at = Instant::now();
            let operation = executor
                .submit(runner.clone(), fixture(), CommandRunOptions::new())
                .expect("submit batch command");
            (operation, submitted_at)
        })
        .collect::<Vec<_>>();
    let mut queue = Vec::with_capacity(count);
    let mut run = Vec::with_capacity(count);
    let mut outcome_at = Vec::with_capacity(count);
    for (operation, submitted_at) in &mut operations {
        let outcome = operation.wait_outcome().expect("batch outcome");
        let observed_at = Instant::now();
        let run_elapsed = match outcome {
            OperationOutcome::Succeeded(ref output) => output.elapsed(),
            other => panic!("batch command failed: {other:?}"),
        };
        queue.push(
            observed_at
                .duration_since(*submitted_at)
                .saturating_sub(run_elapsed),
        );
        run.push(run_elapsed);
        outcome_at.push(observed_at);
        let _ = black_box(outcome);
    }
    let mut settlement = Vec::with_capacity(count);
    for ((operation, _), observed_at) in operations.iter_mut().zip(outcome_at) {
        operation.wait_settlement().expect("batch settlement");
        settlement.push(Instant::now().duration_since(observed_at));
    }
    report_distribution(format!("capture_batch/{count}/queue"), &mut queue);
    report_distribution(format!("capture_batch/{count}/run"), &mut run);
    report_distribution(format!("capture_batch/{count}/settlement"), &mut settlement);
}

fn cancel_batch_once(executor: &CommandExecutor) {
    let runner = CommandRunner::without_timeout();
    let mut operations = (0..8)
        .map(|_| {
            let submitted_at = Instant::now();
            let operation = executor
                .submit(runner.clone(), sleeping_fixture(), CommandRunOptions::new())
                .expect("submit cancellation batch");
            (operation, submitted_at)
        })
        .collect::<Vec<_>>();
    let mut stopped_at = vec![None; operations.len()];
    for (index, (operation, _)) in operations.iter().enumerate().skip(1) {
        black_box(operation.request_stop(StopReason::UserCancelled));
        stopped_at[index] = Some(Instant::now());
    }
    for (operation, _) in &mut operations {
        let _ = black_box(operation.wait_outcome().expect("cancel outcome"));
    }
    let mut cancellation = Vec::new();
    for (index, (operation, _)) in operations.iter_mut().enumerate() {
        operation.wait_settlement().expect("cancel settlement");
        let settled_at = Instant::now();
        if let Some(requested_at) = stopped_at[index] {
            cancellation.push(settled_at.duration_since(requested_at));
        }
    }
    report_distribution(
        "cancel_batch_8/request_to_settlement".to_owned(),
        &mut cancellation,
    );
}

fn managed_execution(c: &mut Criterion) {
    let runner = CommandRunner::without_timeout();
    let managed = executor(4, 64);

    let mut single = c.benchmark_group("managed_execution_single");
    single.bench_function("legacy_sync_capture_0_11_api", |b| {
        b.iter(|| black_box(runner.run(fixture()).expect("legacy command")))
    });
    single.bench_function("managed_sync_capture", |b| {
        b.iter(|| {
            black_box(
                runner
                    .run_managed(&managed, fixture(), CommandRunOptions::new())
                    .expect("managed command"),
            )
        })
    });
    single.bench_function("managed_async_capture", |b| {
        b.iter(|| {
            black_box(
                block_on(runner.run_async(&managed, fixture(), CommandRunOptions::new()))
                    .expect("async command"),
            )
        })
    });
    single.bench_function("managed_stream_two_pipes", |b| {
        b.iter(|| stream_once(&managed))
    });
    single.finish();

    let mut concurrency = c.benchmark_group("managed_execution_concurrency");
    for count in [1, 4, 16, 64] {
        concurrency.bench_with_input(BenchmarkId::new("capture_batch", count), &count, |b, n| {
            b.iter(|| batch_once(&managed, *n))
        });
    }
    concurrency.finish();

    c.bench_function("managed_execution_cancel_recovery_batch_8", |b| {
        b.iter(|| cancel_batch_once(&managed))
    });
}

criterion_group!(benches, managed_execution);
criterion_main!(benches);
