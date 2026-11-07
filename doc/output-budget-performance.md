# Total output budget performance evidence

## Status

The Windows fixtures and measurements are complete. Three isolated Criterion
samples per scenario show that total-output-budget mode has substantially
higher benchmark-process CPU use under idle polling, while adaptive polling
reduces that cost without a measurable end-to-end wall-time regression. The
Windows-only change uses 1, 2, 4, 8, then 10 ms delays and remains capped at
10 ms. Windows cancellation probes pass; RC-002 remains unresolved as recorded
in `doc/rc-002-verification.md`.

## Source and environment

| Run | Source SHA | Windows environment | Result |
| --- | --- | --- | --- |
| Baseline, before adaptive output polling | `11d4654282591383ac86cc62badaec0329358418` | `windows-2022`, Windows 10.0.20348, AMD EPYC 7763, 4 visible processors, Rust/Cargo 1.94.0 | Criterion and process CPU data complete. xperf analysis failed because `-o` followed `-a`; this run is used only for Criterion and parent-process CPU. [Run 42345499130](https://github.com/qubit-ltd/rs-command/actions/runs/42345499130), [artifact 13564175150](https://github.com/qubit-ltd/rs-command/actions/runs/42345499130/artifacts/13564175150). |
| Adaptive polling candidate | `2506ff496f54de1b991ea0ed161f9bf66556327e` | `windows-2022`, Windows 10.0.20348, AMD EPYC 7763, 4 visible processors, Rust/Cargo 1.94.0 | Criterion, process CPU, WPR, xperf context-switch action, and sampled CPU analysis completed. [Run 42349303349](https://github.com/qubit-ltd/rs-command/actions/runs/42349303349), [artifact 13565128121](https://github.com/qubit-ltd/rs-command/actions/runs/42349303349/artifacts/13565128121). |

The two compared runs report the same CPU model and processor count. A separate
baseline run on an Intel Xeon runner was excluded from the comparison because
its hardware differed. An earlier run that lacked Criterion's `--bench` flag
ran Criterion in test mode and is excluded from all performance conclusions.

## Fixture and method

`benches/command_runner.rs` runs a two-second silent child or an intermittent
child that writes and flushes one byte to each stream every 100 ms for 20
iterations. It tests one and eight concurrently released commands. Each runner
has a 10-second timeout, 1024-byte per-stream retention, and either no total
budget or a 1024-byte total budget. Every iteration checks successful exit,
exact stream bytes and lengths, complete streams, and no truncation.

Each of the eight cases was run three times in a separate Criterion process
with `--bench --exact --sample-size 10 --warm-up-time 1 --measurement-time 2`.
All 24 Criterion processes exited 0. The tables below retain each raw Criterion
point estimate and each parent benchmark process `TotalProcessorTime` sample;
the artifacts contain the full Criterion intervals and command logs. Child
fixture processes are separate and excluded from the parent process CPU value.

## Criterion results

Each cell lists the three Criterion point estimates in seconds, followed by
their median in parentheses.

| Scenario | Baseline, SHA `11d4654` | Adaptive polling, SHA `2506ff4` |
| --- | --- | --- |
| Silent, 1 command, no total budget | 2.0329, 2.0231, 2.0215 (**2.0231**) | 2.0249, 2.0264, 2.0324 (**2.0264**) |
| Silent, 1 command, 1024-byte total budget | 2.0255, 2.0237, 2.0260 (**2.0255**) | 2.0310, 2.0261, 2.0232 (**2.0261**) |
| Silent, 8 commands, no total budget | 2.1317, 2.1331, 2.1429 (**2.1331**) | 2.1459, 2.1497, 2.1450 (**2.1459**) |
| Silent, 8 commands, 1024-byte total budget | 2.1547, 2.1416, 2.1461 (**2.1461**) | 2.1584, 2.1674, 2.1452 (**2.1584**) |
| Intermittent, 1 command, no total budget | 2.0444, 2.0387, 2.0303 (**2.0387**) | 2.0306, 2.0326, 2.0302 (**2.0306**) |
| Intermittent, 1 command, 1024-byte total budget | 2.0352, 2.0340, 2.0351 (**2.0351**) | 2.0326, 2.0353, 2.0408 (**2.0353**) |
| Intermittent, 8 commands, no total budget | 2.1528, 2.1399, 2.1578 (**2.1528**) | 2.1505, 2.1499, 2.1506 (**2.1505**) |
| Intermittent, 8 commands, 1024-byte total budget | 2.1664, 2.1723, 2.1666 (**2.1666**) | 2.1535, 2.1446, 2.1515 (**2.1515**) |

The candidate wall-time medians range from 0.7% below to 0.6% above baseline.
The 10 ms maximum polling delay is 10% of the fixture's 100 ms intermittent
write interval. The benchmark records end-to-end completion, not per-write
delivery timestamps, so it does not measure individual write-to-read latency.

## Parent benchmark process CPU

Each cell lists the three raw `TotalProcessorTime` samples in seconds and their
median.

| Scenario | Baseline samples (median) | Candidate samples (median) | Median CPU change |
| --- | --- | --- | ---: |
| Silent, 1 command, no total budget | 0.531, 0.250, 0.328 (**0.328**) | 0.234, 0.250, 0.203 (**0.234**) | −28.7% |
| Silent, 1 command, 1024-byte total budget | 1.984, 2.438, 2.109 (**2.109**) | 0.531, 0.484, 0.625 (**0.531**) | −74.8% |
| Silent, 8 commands, no total budget | 4.594, 4.500, 4.828 (**4.594**) | 5.188, 5.234, 5.016 (**5.188**) | +12.9% |
| Silent, 8 commands, 1024-byte total budget | 11.781, 10.344, 11.781 (**11.781**) | 5.766, 5.719, 5.406 (**5.719**) | −51.5% |
| Intermittent, 1 command, no total budget | 0.234, 0.344, 0.234 (**0.234**) | 0.172, 0.281, 0.172 (**0.172**) | −26.5% |
| Intermittent, 1 command, 1024-byte total budget | 2.000, 1.703, 2.062 (**2.000**) | 0.750, 0.516, 0.625 (**0.625**) | −68.8% |
| Intermittent, 8 commands, no total budget | 4.859, 4.562, 5.453 (**4.859**) | 4.734, 4.812, 4.812 (**4.812**) | −1.0% |
| Intermittent, 8 commands, 1024-byte total budget | 12.469, 11.844, 12.047 (**12.047**) | 5.609, 6.141, 6.172 (**6.141**) | −49.0% |

Budget-mode parent CPU fell in all four budgeted scenarios. In the two
eight-command cases, median CPU use fell by about one half. Unbudgeted CPU use
was broadly similar; the silent eight-command case varied upward by 12.9%,
which is retained above rather than hidden. This repeated, same-model result
supports keeping the local adaptive backoff.

## ETW, cancellation, and limits

The candidate run recorded WPR `GeneralProfile` and stopped it successfully.
It then ran:

```text
wpr -start GeneralProfile -filemode
wpr -stop performance-results/output-budget.etl
xperf -i performance-results/output-budget.etl -o performance-results/context-switches.txt -a cswitch
xperf -i performance-results/output-budget.etl -o performance-results/sampled-cpu.txt -a profile
```

All four commands completed successfully. The xperf `cswitch` report is the
tool's CPU-availability summary derived from context-switch events; the
sampled-profile report is system-wide CPU utilization. Neither report gives
per-thread wakeup counts, so no exact wakeup-count claim is made. The workflow
retains these text reports and Criterion logs; it excludes the 1.6 GB ETL file
from the artifact.

The candidate's Windows cancellation/interleaving job passed all 12 required
test identities, including the real-cancel read-to-tee, readiness, EOF-to-flush,
stdin-check-to-write, and production command-I/O probes. The production
read-to-tee trace still returned OS error 995 on the original read, with no
subsequent I/O, no pending production join witness, and no watchdog release.
RC-002 therefore remains `unresolved`. The readiness backoff checks cancellation
at every loop entry and returns immediately when data is available. Its
per-probe delay sequence and 10 ms cap have a deterministic unit test. Exact
wall-clock cancellation latency and per-write delivery latency were not
instrumented; the maximum extra wait before the next readiness/cancellation
check is bounded by 10 ms.
