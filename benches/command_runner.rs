// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Benchmarks short-lived command execution with and without timeout handling.

use std::hint::black_box;
use std::time::Duration;

use criterion::Criterion;
use criterion::criterion_group;
use qubit_command::Command;
use qubit_command::CommandError;
use qubit_command::CommandErrorReason;
use qubit_command::CommandOutput;
use qubit_command::CommandRunner;

const SILENT_FIXTURE_ARG: &str = "--qubit-command-silent-fixture";
const SILENT_FIXTURE_DELAY: Duration = Duration::from_millis(10);

/// Builds a shell command that exits successfully without producing output.
///
/// # Returns
///
/// A portable short-lived shell command for command-runner benchmarks.
fn short_lived_command() -> Command {
    Command::shell("exit 0")
}

/// Builds a command that runs this benchmark executable's silent fixture.
fn silent_command() -> Command {
    Command::new_os(std::env::current_exe().expect("benchmark executable should resolve")).arg(SILENT_FIXTURE_ARG)
}

/// Builds a deterministic command that writes a fixed number of zero bytes.
fn output_command(bytes: usize) -> Command {
    #[cfg(unix)]
    {
        Command::shell(&format!("head -c {bytes} /dev/zero"))
    }
    #[cfg(windows)]
    {
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(&format!(
                "$b = New-Object byte[] {bytes}; [Console]::OpenStandardOutput().Write($b, 0, $b.Length)"
            ))
    }
}

/// Verifies that a runner can execute the benchmark fixture before timing it.
///
/// # Parameters
///
/// * `runner` - Runner configuration measured by the benchmark.
///
/// # Panics
///
/// Panics when the local shell cannot execute the benchmark fixture.
fn verify_fixture(runner: &CommandRunner) {
    let output = runner
        .run(short_lived_command())
        .expect("short-lived benchmark command should succeed");
    let _ = black_box(output);
}

/// Measures the default timeout path against the explicit no-timeout path.
///
/// # Parameters
///
/// * `criterion` - Criterion benchmark registry receiving both scenarios.
fn benchmark_short_lived_command_runner(criterion: &mut Criterion) {
    let default_timeout_runner = CommandRunner::new(Duration::from_secs(10));
    let without_timeout_runner = CommandRunner::without_timeout();
    verify_fixture(&default_timeout_runner);
    verify_fixture(&without_timeout_runner);

    let mut group = criterion.benchmark_group("short_lived_command_runner");
    group.bench_function("default_timeout", |bencher| {
        let mut failures = 0;
        bencher.iter(|| {
            black_box(
                default_timeout_runner
                    .run(black_box(short_lived_command()))
                    .inspect_err(|_| failures += 1),
            )
        });
        assert_eq!(failures, 0, "benchmark commands must succeed");
    });
    group.bench_function("without_timeout", |bencher| {
        let mut failures = 0;
        bencher.iter(|| {
            black_box(
                without_timeout_runner
                    .run(black_box(short_lived_command()))
                    .inspect_err(|_| failures += 1),
            )
        });
        assert_eq!(failures, 0, "benchmark commands must succeed");
    });
    group.finish();
}

/// Measures process startup, a fixed silent wait, and runner completion end to
/// end.
fn benchmark_silent_command_runner(criterion: &mut Criterion) {
    let runner = CommandRunner::new(Duration::from_secs(10));
    let command = silent_command();
    let output = runner.run(command.clone()).expect("silent fixture should succeed");
    assert!(output.stdout().is_empty(), "silent fixture stdout should be empty");
    assert!(output.stderr().is_empty(), "silent fixture stderr should be empty");
    assert!(output.stdout_complete() && output.stderr_complete());
    assert!(!output.stdout_truncated() && !output.stderr_truncated());
    assert!(output.elapsed() >= SILENT_FIXTURE_DELAY);

    let mut group = criterion.benchmark_group("silent_command_runner");
    group.bench_function("default_timeout", |bencher| {
        let mut failures = 0;
        bencher.iter(|| black_box(runner.run(black_box(command.clone())).inspect_err(|_| failures += 1)));
        assert_eq!(failures, 0, "benchmark commands must succeed");
    });
    group.finish();
}

/// Measures successful and over-budget capture of large, controlled output.
fn benchmark_total_output_budget(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("total_output_budget");
    for retained in [1024 * 1024, 16 * 1024 * 1024] {
        for (scenario, emitted) in [("within_limit", retained), ("limit_plus_one", retained + 1)] {
            let runner = CommandRunner::without_timeout()
                .max_output_bytes(retained)
                .max_total_output_bytes(retained as u64);
            let command = output_command(emitted);
            let result = runner.run(command.clone());
            let over_limit = scenario == "limit_plus_one";
            assert!(
                classifies_budget_result(&result, over_limit, retained as u64),
                "fixture result should match {scenario}"
            );
            if !over_limit {
                let output = result.as_ref().expect("in-limit fixture should succeed");
                assert!(
                    output.stdout().iter().all(|byte| *byte == 0),
                    "fixture should contain only zero bytes"
                );
                assert_eq!(
                    output.stdout().len(),
                    emitted,
                    "fixture should have expected stdout length"
                );
                assert_eq!(output.stderr().len(), 0, "fixture should not write stderr");
                assert!(
                    output.stdout().len() <= retained,
                    "capture should respect retention cap"
                );
            } else {
                let error = result.as_ref().expect_err("over-limit fixture should fail");
                let captured = error
                    .captured_output()
                    .expect("budget failure should retain its output snapshot");
                assert!(
                    captured.stdout().iter().all(|byte| *byte == 0),
                    "fixture should contain only zero bytes"
                );
                assert!(
                    captured.stderr().iter().all(|byte| *byte == 0),
                    "fixture should contain only zero bytes"
                );
                assert!(
                    captured.stdout().len() <= retained,
                    "stdout capture should respect retention cap"
                );
                assert!(
                    captured.stderr().len() <= retained,
                    "stderr capture should respect retention cap"
                );
            }
            let mut unexpected = 0;
            group.bench_function(format!("{retained}_{scenario}"), |bencher| {
                bencher.iter(|| {
                    let result = runner.run(black_box(command.clone()));
                    if !classifies_budget_result(&result, over_limit, retained as u64) {
                        unexpected += 1;
                    }
                    black_box(result)
                });
            });
            assert_eq!(unexpected, 0, "benchmark iterations must match {scenario}");
        }
    }
    group.finish();
}

/// Classifies one benchmark result without scanning captured stream contents.
fn classifies_budget_result(result: &Result<CommandOutput, CommandError>, over_limit: bool, limit: u64) -> bool {
    if over_limit {
        matches!(
            result,
            Err(error)
                if matches!(error.reason(), CommandErrorReason::OutputLimitExceeded { limit: actual_limit, observed }
                    if *actual_limit == limit && *observed == u128::from(limit) + 1)
                    && error.output().is_none()
        )
    } else {
        matches!(result, Ok(output)
            if output.stdout().len() == limit as usize
                && output.stderr().is_empty()
                && output.stdout_complete()
                && output.stderr_complete()
                && !output.stdout_truncated()
                && !output.stderr_truncated())
    }
}

criterion_group!(
    benches,
    benchmark_short_lived_command_runner,
    benchmark_silent_command_runner,
    benchmark_total_output_budget,
);

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == SILENT_FIXTURE_ARG {
        std::thread::sleep(SILENT_FIXTURE_DELAY);
        return;
    }
    benches();
    Criterion::default().configure_from_args().final_summary();
}
