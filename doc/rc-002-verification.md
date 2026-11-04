# RC-002 Windows Cancellation Interleaving Verification

## Status

**Confirmed: the Windows read-to-tee cancellation interleaving reproduced.**
The controlled harness observed a successful real `CancelSynchronousIo` return
before the helper entered a new blocked synchronous tee write. A narrow
production fix and deterministic regression test are now being validated. The
reproduction was on the harness commit before that fix; the next Windows CI run
must confirm the new cancellation check prevents the schedule.

## Environment

- Host: Linux x86_64 (`Linux 6.8.0-147-generic`)
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`
- Baseline commit: `d09a6624f10790d2bc0b171c919ee18f83105586`
- Direct Windows execution from the active workspace: unavailable
- Relevant repository CI: `.github/workflows/ci.yml` includes `windows-2022`,
  and was used for the committed changes.
- GitHub Actions run: [Rust CI run 41689180683](https://github.com/qubit-ltd/rs-command/actions/runs/41689180683),
  commit `ab636a295c717fa3bbd2f23dc5ac95614aabf06b`; `rust-ci / verify
  (windows-2022)` completed successfully on 2026-11-03.
- Planned Windows interleaving command:
  `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`
  was not run because that test is not present in the repository. The Windows
  CI job ran existing tests and checks, not the planned controlled interleaving.

## Source Trace

- `internal/io_cancellation.rs::IoCancellation::cancel` sets the cancellation
  token before requesting platform cancellation.
- On Windows, `internal/cancel.rs::cancel_synchronous_io` calls
  `CancelSynchronousIo`. A successful call returns `Ok(())`; error 1168
  (`ERROR_NOT_FOUND`) and other errors remain distinguishable.
- `internal/command_io.rs::finish_helpers` requests cancellation for stdout,
  stderr, and stdin before finishing helpers. It uses one shared 100 ms
  confirmation deadline only when a cancellation request returned an error.
  A successful request proceeds directly to `join`, which has no deadline.
- `internal/output_collector.rs::read_output_inner` checks cancellation before
  the next readiness/read operation. After a successful read it retains bytes,
  then may write them to the tee before checking the token again at the next
  loop iteration. EOF is recorded before the optional tee flush; `complete`
  therefore remains true if cancellation arrives after EOF.
- `internal/stdin_pipe.rs::write_stdin_until_cancelled` checks cancellation,
  calls `wait_writable`, and then writes. The Windows `wait_writable`
  implementation checks the token, leaving a check-to-write interval.

These source paths identify the interleavings to exercise. They do not establish
that Windows `CancelSynchronousIo` can return success while the operation
completes normally and the helper proceeds into a later synchronous I/O call.
That operating-system behavior is the unresolved point.

## Test Evidence and Limits

The Linux CI run passed the existing `command_io` and cancellation tests,
including `test_finish_helpers_joins_when_all_cancellations_succeed`. That test
uses already completed helpers and does not cover a live Windows synchronous
I/O completion race. Existing cancellation-failure tests likewise do not
substitute for the successful-cancellation interleaving.

No controlled reader/tee/stdin interleaving test was added. A fake worker that
ignores cancellation would only prove the join strategy, not the Windows
operating-system race. The successful Windows CI run above had no test that
exercised barrier-controlled read-to-tee, EOF-to-flush, or stdin check-to-write
schedules with a real `CancelSynchronousIo` result and watchdog release path.
Those experiments and their exact test results remain outstanding.

## Current Investigation Attempt

- Worktree branch: `codex/rc001-rc002-implementation-20261103`, based on
  `9f0f306`; RC-001 was committed as `10f0ac0`. The task branch has no matching
  ref on `origin` yet.
- The active host is Linux x86_64. The repository's Windows 2022 GitHub Actions
  workflow supports `workflow_dispatch`, so a branch run is available after
  the controlled test exists and the branch is published. No such run was
  dispatched because the required interleaving test is not present. The
  earlier Windows job therefore remains unrelated to this test.
- The repository has no controlled Windows interleaving test or barriers at
  the read-to-tee, EOF-to-flush, or stdin check-to-write boundaries. Adding a
  test that merely holds a helper after a successful cancellation would test
  the join policy, not whether Windows cancellation can be followed by a new
  synchronous I/O call. I did not add such a test or claim it as evidence.
- There is no existing test seam that can establish the required OS state.
  Pausing after `Read::read` returns, after EOF, or after `wait_writable`
  returns means the worker is between synchronous I/O calls; a cancellation
  request at that point cannot be evidence that `CancelSynchronousIo` returned
  success for the preceding operation. Pausing before the next call and then
  setting the token also makes the helper's normal cancellation checks stop it
  before entering that call. Reaching the required schedule therefore needs a
  race between a real Windows I/O completion and `CancelSynchronousIo`, with
  instrumentation that records the actual system result and whether the next
  real I/O call began. No such instrumentation or watchdog/release harness is
  present. A test-only blocking fake would bypass the OS behavior under review
  and was not used.
- A best-effort stress loop over real pipes would be probabilistic: it cannot
  guarantee that the completion/cancellation race occurs, and zero hits would
  leave the same evidence gap. Introducing scheduler barriers cannot make that
  schedule deterministic for the reasons above. No such probe was added; the
  test design must not report a pass unless it observes a valid schedule.
- No Windows process was available locally. The Windows CI runner can execute
  a branch test once implemented and published, but the exact planned Windows
  commands remain unrun. No event trace, watchdog result, or helper-reap result
  was collected for these schedules.

## Current Implementation Validation

The implementation branch is `codex/rc001-rc002-implementation-20261103` at
`10f0ac0` for the completed RC-001 commit. On Linux x86_64 with
`rustc 1.94.0 (4a4ef493e 2026-03-02)`, the following completed successfully:

- `.infra/bin/align-ci.sh` and `.infra/bin/style-check.sh`
- `cargo test --lib budget_only_timer` (2 passed, 89 filtered)
- `cargo test --all-targets` (exit code 0)
- `cargo test --doc` (15 passed)
- `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps`
- `.infra/bin/ci-check.sh` (exit code 0, including style, clippy, verify,
  strict docs, feature matrix, package, coverage, and audit)

These checks validate RC-001 and the Linux project baseline. They do not
resolve RC-002 or substitute for its Windows interleaving evidence.

## Decision

The controlled Windows probe confirmed that a helper can continue into a
blocking tee write after `CancelSynchronousIo` succeeds on the preceding read.
The repair now checks the cancellation token after retaining the bytes and
before starting that tee write. This keeps bytes already read, leaves the
stream incomplete, and allows the helper to stop without entering another
synchronous operation. The deterministic regression verifies those results.
The fix still requires a passing Windows CI run; zero hits on a post-fix probe
will remain limited evidence because the schedule is race-driven.

## Windows CI Reproduction

`src/command_runner/internal/output_collector.rs` now contains the Windows-only
test `test_windows_real_cancel_read_to_blocked_tee_interleaving`. Each attempt
uses real anonymous Windows pipes backed by owning `File` handles. It fills
the tee pipe and checks its queued byte count, then starts the real
`read_output_inner` worker on a blocking source-pipe read. After observing the
read wrapper's entry event, a barrier releases a source-pipe write and the
controller's real `IoCancellation::cancel` concurrently.

An attempt is counted as a valid hit only if all of these events are observed
in that attempt: the first real cancellation returns success; its return
timestamp precedes the tee writer's event immediately before its actual
`File::write`; the worker remains unfinished for the bounded observation
interval; and a second real `CancelSynchronousIo` call succeeds, confirming a
synchronous operation is pending on that worker. The tee pipe was verified
full before the attempt. Every attempt closes/releases the source writer,
drains the tee reader to EOF, and joins the workers before continuing. Bounded
event receives prevent a missing transition from waiting forever. The test
prints attempt counts, cancellation OS error codes, and the hit timeline.

The probe calls `read_output_inner` with `fd=None` to exercise an actual
blocking `File::read`; this bypasses Windows' outer `PeekNamedPipe` readiness
loop. It therefore reproduces the real read-to-tee core interleaving but does
not establish that the full production scheduling path follows the same
schedule. A zero-hit run remains unverified. A valid hit fails the test with
the trace so the production path can be repaired before CI is accepted.

The first targeted run was GitHub Actions
[Rust CI run 41740883288](https://github.com/qubit-ltd/rs-command/actions/runs/41740883288),
commit `92c39123856df37f85d9b109d15cd6534189e275`, on `windows-2022`. It
reported 3 attempts, 3 successful first cancellations, 0 `ERROR_NOT_FOUND`
results, 2 tee write entries, 1 confirmed blocked write, and 1 valid hit on
attempt 2. The hit required a second successful real `CancelSynchronousIo`
while the worker remained unfinished after the bounded observation interval.
The timeline was initially printed in milliseconds, which rounded all event
offsets to zero; the harness now records nanosecond offsets for subsequent runs.
The Windows test failed intentionally on the confirmed hit. On the post-fix
commit `8fa796a`, Windows CI run
[41742850551](https://github.com/qubit-ltd/rs-command/actions/runs/41742850551)
passed all 99 library tests, including the interleaving test. Since the test
passes only when its valid-hit list remains empty after all 128 attempts, this
run found no valid post-fix hit. The overall Windows job then failed in Clippy
because the harness retained an unused `WriteFile` declaration; that
declaration has been removed, and a full CI rerun is pending. Ubuntu and macOS
passed this workflow. The earlier macOS watchdog failure was transient and did
not recur.

## Windows CI 已复现交错

`src/command_runner/internal/output_collector.rs` 新增 Windows 专用测试
`test_windows_real_cancel_read_to_blocked_tee_interleaving`。每次尝试都使用
由 `File` 所有的真实 Windows 匿名管道。测试先填满 tee 管道并核对其中的字节数，
再启动真实的 `read_output_inner` 工作线程，使其阻塞读取 source 管道。观察到
read wrapper 的入口事件后，通过 barrier 同时放行 source 管道写入和控制线程对
真实 `IoCancellation::cancel` 的调用。

只有同一次尝试同时满足以下条件才计为有效命中：第一次真实取消返回成功；其返回
时间早于 tee writer 在实际 `File::write` 前发送的事件；观察窗口内 worker 仍未结束；
并且对该 worker 再调用一次真实 `CancelSynchronousIo` 成功，确认其同步 I/O 当时
仍处于 pending 状态。测试开始前已验证 tee 管道填满。每次尝试都会释放 source
写端、将 tee 读端排空至 EOF，并 join worker；等待事件均有超时。测试会输出尝试数、
取消错误码和命中时间线。

当前探针使用 `read_output_inner(..., fd=None)`，以便真实 `File::read` 阻塞；这会
绕过 Windows 外层的 `PeekNamedPipe` readiness 循环。因此它复现了真实 read-to-tee
核心交错，但不证明完整生产调度路径一定采用相同顺序。零命中仍表示待验证。有效
命中会带时间线使测试失败，以便在接受 CI 前修复生产路径。

首次目标运行是 GitHub Actions
[Rust CI run 41740883288](https://github.com/qubit-ltd/rs-command/actions/runs/41740883288)，
提交 `92c39123856df37f85d9b109d15cd6534189e275`，平台为 `windows-2022`。结果为
3 次尝试、3 次首次取消成功、0 次 `ERROR_NOT_FOUND`、2 次 tee 写入口、1 次确认
阻塞写，并在第 2 次尝试中出现 1 次有效命中。有效命中还要求有界观察窗口后 worker
仍未结束，且第二次真实 `CancelSynchronousIo` 也成功。初版时间线以毫秒打印，所有
偏移都被舍入为零；后续运行已改为记录纳秒偏移。Windows 测试因命中而按设计失败。

修复后的提交 `8fa796a` 在 Windows CI run
[41742850551](https://github.com/qubit-ltd/rs-command/actions/runs/41742850551)
中通过了全部 99 个库测试，包括该交错测试。根据测试断言，只有 128 次尝试后有效
命中列表仍为空才会通过，因此本次修复后运行没有有效命中。整条 Windows job 随后在
Clippy 阶段因测试 harness 中未使用的 `WriteFile` 声明失败；该声明已移除，完整 CI
重跑等待中。同一 workflow 的 Ubuntu 和 macOS 均通过。先前 macOS 上既有
`stdin_hold_is_cancelled_within_watchdog` 测试的瞬时失败未再次出现。
