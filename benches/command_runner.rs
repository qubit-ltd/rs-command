// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Benchmarks short-lived command execution with and without timeout handling.

use std::hint::black_box;

use criterion::{
    Criterion,
    criterion_group,
    criterion_main,
};
use qubit_command::{
    Command,
    CommandRunner,
};

/// Builds a shell command that exits successfully without producing output.
///
/// # Returns
///
/// A portable short-lived shell command for command-runner benchmarks.
fn short_lived_command() -> Command {
    Command::shell("exit 0")
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
    let default_timeout_runner = CommandRunner::new();
    let without_timeout_runner = CommandRunner::new().without_timeout();
    verify_fixture(&default_timeout_runner);
    verify_fixture(&without_timeout_runner);

    let mut group = criterion.benchmark_group("short_lived_command_runner");
    group.bench_function("default_timeout", |bencher| {
        bencher.iter(|| {
            black_box(
                default_timeout_runner.run(black_box(short_lived_command())),
            )
        });
    });
    group.bench_function("without_timeout", |bencher| {
        bencher.iter(|| {
            black_box(
                without_timeout_runner.run(black_box(short_lived_command())),
            )
        });
    });
    group.finish();
}

criterion_group!(benches, benchmark_short_lived_command_runner);
criterion_main!(benches);
