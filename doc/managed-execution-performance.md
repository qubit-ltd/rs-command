# Managed execution performance

This benchmark compares the retained synchronous `CommandRunner::run` path with managed synchronous collection, async collection, and a two-pipe managed session. The same portable `cmd`/`sh` fixture is used for all single-command cases. Batch cases use 1, 4, 16, and 64 accepted commands; the last case fills the manager's default child-operation capacity while exercising the executor's active/queue admission path. A separate batch requests cancellation for seven of eight submitted short-lived commands and waits for both outcomes and settlement.

## Reproduction

Run on an otherwise idle host, with the same Rust toolchain and machine for the 0.11.2 baseline and 0.12.0 candidate:

```sh
rustc --version
cargo bench --features async --bench managed_execution
```

Criterion reports per-case latency distributions and confidence intervals. Run the baseline from the 0.11.2 source revision with the equivalent legacy case; managed cases do not exist in that release. Capture child-inclusive peak resident memory with `/usr/bin/time -v` on Linux. Record peak process/thread counts during the concurrency and cancellation groups with an OS process monitor; report those measurements separately because Criterion does not collect them. Do not compare runs made under concurrent load.

## Measurements

Measurements below were collected on Linux x86_64, Ubuntu kernel 6.8.0-147-generic, Intel Xeon E5-2680 v4 (56 logical CPUs), Rust 1.94.0. Both revisions used Criterion sample size 20, 1 second warm-up, and 2 seconds measurement time. The fixture writes the same short string using `sh -c "printf managed-output"`. The 0.11.2 baseline used commit `5a754f41a9ef2c7c0576f3ebdb0d1297d87c5e0b`; the 0.12 candidate used the task's integrated implementation before final repository alignment. Intervals are Criterion's 95% estimates. These are short-lived process measurements with a small sample, so overlapping intervals do not support a speedup claim.

| Revision | Case | Median | 95% interval |
| --- | --- | ---: | ---: |
| 0.11.2 | Legacy synchronous capture | 1.685 ms | 1.639–1.734 ms |
| 0.12.0 | Legacy synchronous capture | 1.685 ms | 1.637–1.742 ms |
| 0.12.0 | Managed synchronous capture | 10.887 ms | 10.829–10.937 ms |
| 0.12.0 | Managed async capture | 10.790 ms | 10.748–10.825 ms |
| 0.12.0 | Managed two-pipe session | 10.903 ms | 10.886–10.921 ms |
| 0.12.0 | Capture batch, 1 | 10.807 ms | 10.753–10.854 ms |
| 0.12.0 | Capture batch, 4 | 11.166 ms | 11.112–11.237 ms |
| 0.12.0 | Capture batch, 16 | 43.681 ms | 43.581–43.780 ms |
| 0.12.0 | Capture batch, 64 | 172.680 ms | 172.440–172.960 ms |
| 0.12.0 | Stop and settlement batch, 8 | 41.484 ms | 41.440–41.529 ms |

Whole-process resource measurements for the measured Criterion run were 43,456 KiB peak RSS for 0.11.2 and 44,800 KiB for 0.12.0 (`/usr/bin/time -v`, including the Cargo/Criterion process). A process-tree sample during a one-pass 0.12.0 smoke run observed 35 peak threads and 48,848 KiB aggregate RSS. The legacy-only baseline had no persistent executor; its transient child/helper thread count was not captured reliably by the 5 ms sampler. The candidate uses four command workers and caps command admission at four active plus 64 queued; its measured process tree includes the operation manager workers, recovery worker, benchmark harness, and child output helpers. Native Windows and macOS measurements require their respective CI runners; Linux results do not stand in for those platforms.

The one-pass reporting mode also records operation-level timings. Queue is an approximation computed as submit-to-outcome time minus `CommandOutput::elapsed()`; settlement is measured after outcome until `wait_settlement()` returns. These distributions are per-operation samples from a single batch, not Criterion confidence intervals:

| Accepted batch size | Queue p50 / p95 | Process run p50 / p95 | Post-outcome settlement p50 / p95 |
| ---: | ---: | ---: | ---: |
| 1 | 0.426 / 0.426 ms | 10.208 / 10.208 ms | 0.001 / 0.001 ms |
| 4 | 0.408 / 0.413 ms | 10.226 / 10.292 ms | 0.308 / 0.462 ms |
| 16 | 10.897 / 31.319 ms | 10.185 / 10.282 ms | 10.887 / 31.697 ms |
| 64 | 73.342 / 146.849 ms | 10.184 / 10.267 ms | 75.317 / 149.862 ms |

For the eight-command stop batch, the seven requested stops reached settlement in 40.536 ms p50 and 40.547 ms p95. This workload waits for all eight outcomes before releasing the handles and waiting for their settlements, so cancellation latency includes that observation order.

The reported Criterion intervals measure end-to-end operation latency. Queue split values are approximate because they include submission and observer overhead. The two revisions' legacy capture intervals overlap, so the data shows no material change to the retained synchronous path on this fixture. Managed execution has measurable setup and lifecycle overhead; the 64-command batch remains bounded and completed in about 173 ms on this host.
