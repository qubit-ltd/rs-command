# Total output budget performance evidence

## Status

The benchmark fixtures are implemented, and both required Criterion smoke
commands passed on 2026-11-07. Windows performance measurements have not been
captured. This document records the runnable coverage and the outstanding
measurement work; it does not claim a Windows performance result or justify a
production polling change.

## Source and environment

- Repository baseline before the benchmark edit: `9304afb53679f11f3707753d9b9a396baa78d448`.
- The benchmark edit is uncommitted in the `cmd-validation-20261107` Git
  worktree, so there is no resulting source commit SHA to report.
- Host: Linux x86_64, kernel `6.8.0-147-generic`.
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`.
- Cargo: `cargo 1.94.0 (85eff7c80 2026-01-15)`.
- Windows CPU timing and ETW/WPR tools were unavailable on this host and were
  not run. No Windows sample, CPU figure, wakeup count, or context-switch
  result is available.

## Fixture and scenarios

`benches/command_runner.rs` dispatches child fixtures only when the benchmark
executable receives exactly one recognized fixture argument. The long-silent
fixture sleeps for two seconds. The intermittent fixture repeats 20 times,
sleeping 100 ms and writing/flushing one byte to stdout and one byte to stderr
per iteration. Each runner has a 10-second timeout and a 1024-byte per-stream
retention cap. The compared configurations omit a total budget or set a
1024-byte total budget.

Each scenario covers one and eight concurrent commands. A thread barrier
releases each command group together. Every command result is checked for
successful exit, exact bytes and lengths, complete streams, and no truncation.
These fixture assertions establish correctness for the smoke run; they are not
performance measurements.

## Captured command results

| Command | Exit code | Result |
| --- | ---: | --- |
| `cargo bench --bench command_runner -- idle_output_budget --test` | 0 | All eight silent/intermittent, 1/8-command, budget/no-budget cases succeeded. |
| `cargo bench --bench command_runner -- total_output_budget --test` | 0 | Existing within-limit and limit-plus-one cases passed for both retained limits. |

No Criterion timing samples were captured. These `--test` runs are smoke checks
only and cannot support a CPU or wakeup comparison.

## Windows measurement still required

Run on an otherwise idle Windows host with the benchmark source SHA recorded:

1. Run `cargo bench --bench command_runner -- idle_output_budget` at least three
   times and preserve each raw Criterion result.
2. Record Windows version, hardware, Rust toolchain, and benchmark source SHA.
3. Measure the benchmark process CPU time separately from child fixture CPU,
   and capture parent-process CPU plus thread wakeups/context switches with
   ETW/WPR. Record the actual collection tools and commands; if collection is
   unavailable, state that gap rather than substituting wall time.
4. Summarize each scenario's raw samples and median. Include intermittent output
   and cancellation latency if evaluating a polling change.

Until those measurements show stable additional cost, retain the current
production implementation. Windows performance validation remains incomplete.
