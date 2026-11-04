# RC-002 Windows Cancellation Interleaving Verification

## Result

The Windows-only probe reproduced the reported interleaving before the fix.
On a successful `CancelSynchronousIo` return from a blocking source read, the
worker could proceed into a new synchronous tee write and block there. The
production code now checks the cancellation token after retaining bytes from a
successful read and before starting the tee write. A deterministic regression
test verifies that the bytes remain captured while no tee write begins.

The post-fix Windows probe and the full GitHub Actions workflow passed on
commit `cafda675f7f11cee177f76020794bf2d80a02831`:
[Rust CI run 41745608628](https://github.com/qubit-ltd/rs-command/actions/runs/41745608628).
Its `windows-2022`, `ubuntu-latest`, and `macos-15` verification jobs passed, as
did the workflow's coverage, feature matrix, and configured infrastructure
suites. The Windows probe passed after exhausting all 128 attempts, so it
observed no valid post-fix hit. As a race-driven probe, this is evidence about
the tested schedule, not a proof that every production schedule is impossible.

## Current worktree probe result

The worktree adds `.github/workflows/windows-cancellation-interleaving.yml`. It runs
the named test on `windows-2022`, prints the actual `GITHUB_SHA` into the job log,
and uploads that log with `if: always()`. The full GitHub Actions run for exact
head `e4ca5a3c812b17b12e824b1ee71edcc03b54f7be` passed:
[Rust CI run 41755997380](https://github.com/qubit-ltd/rs-command/actions/runs/41755997380).
Its Windows job ran the named probe successfully and emitted all classification
records below. Across 128 read-to-tee attempts, 26 first cancellations
succeeded, 102 returned `ERROR_NOT_FOUND` (1168), no tee write began, and no
partial candidate was observed. The scenario remains `uncovered` because this
probe does not demonstrate a watchdog-rescued production cleanup join; the
overall RC-002 result remains `unresolved`.

The test first verifies the watchdog with a full real anonymous pipe and a blocked
writer helper; a separate watchdog thread drains the pipe and the helper is
joined before the outer process watchdog deadline. Child termination is only the
outer watchdog's fallback. The test emits JSON Lines records named
`scenario_classification` and `overall_classification`. When all 128 attempts finish
without a partial candidate, the log includes these records:

```json
{"event":"scenario_classification","schema":1,"scenario":"read_to_tee","classification":"uncovered","attempts":128,"partial_probe_hits":0,"reason":"probe calls read_output_inner directly and reaps by a second cancellation plus manual pipe drain; it does not establish that production OutputReader cleanup join requires watchdog rescue"}
{"event":"scenario_classification","schema":1,"scenario":"eof_to_flush","classification":"uncovered","attempts":0,"reason":"production tee is an unbuffered File writer; no buffered bytes remain for a blocking flush after EOF. A BufferWriter/BufWriter would change the production path and cannot establish this interleaving"}
{"event":"scenario_classification","schema":1,"scenario":"stdin_check_to_write","classification":"uncovered","attempts":0,"reason":"Windows wait_writable only checks the cancellation token; there is no pending synchronous I/O in the check-to-write gap for CancelSynchronousIo to successfully cancel. A success would target the write after it entered the kernel"}
{"event":"overall_classification","schema":1,"rc002":"unresolved","reason":"all three planned scenarios lack a complete witness satisfying successful cancellation, normal completion, subsequent blocked I/O, production cleanup join, and watchdog release"}
```

The read-to-tee loop is retained as a partial legacy probe, but it calls
`read_output_inner` directly and reaps by a second cancellation and manual pipe
drain. It does not establish a production `OutputReader` cleanup join blocked
until watchdog release. EOF-to-flush has no buffered tee data in the production
`File` writer path; substituting a `BufWriter` would test a different path. On
Windows, `PollableStdin::wait_writable` only checks the token, so the
check-to-write gap has no pending synchronous operation for a successful
`CancelSynchronousIo` result to identify. These boundaries are explicitly
uncovered, not no-hit results. RC-002 remains unresolved.

Current local checks on Linux: `cargo check --lib` passed; the requested focused
`cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1
--nocapture` exited 0 but discovered zero tests because the probe is Windows-only.
Rustfmt syntax checks with `--emit stdout --config skip_children=true
--edition 2024` and `git diff --check` passed. A Windows-target `cargo check --tests
--target x86_64-pc-windows-gnu` could not reach this crate because the environment
lacks `x86_64-w64-mingw32-gcc`. The required Windows run was performed by GitHub
CI as recorded above.

## What the probe establishes

`src/command_runner/internal/output_collector.rs` contains the Windows-only
test `test_windows_real_cancel_read_to_blocked_tee_interleaving`. It uses real
anonymous Windows pipes, owning `File` handles, a real blocking `File::read`,
and real `IoCancellation::cancel` / `CancelSynchronousIo` calls. It fills the
tee pipe before each attempt, releases the source writer and cancellation
controller concurrently, and records the time of the tee writer's entry just
before its real `File::write`.

The harness reports a valid hit only when the first cancellation succeeds,
returns before tee-write entry, the worker remains unfinished for the bounded
observation interval, and a second real `CancelSynchronousIo` succeeds while
the worker is still pending. The pre-fix CI run
[41740883288](https://github.com/qubit-ltd/rs-command/actions/runs/41740883288)
observed one such hit on attempt 2: three attempts, three successful first
cancellations, zero `ERROR_NOT_FOUND` results, two tee-write entries, and one
confirmed blocked write. That run intentionally failed on the hit, preserving
the evidence before the production fix.

The probe invokes `read_output_inner(..., fd=None)` to exercise a real blocking
read. This bypasses the outer Windows `PeekNamedPipe` readiness loop, so the
result specifically establishes the read-to-tee core interleaving and does
not prove that the full production scheduling path always follows that order.
After the fix, the probe passed all 128 attempts with no valid hit. The fix's
deterministic regression test separately verifies that a cancellation
observed after reading preserves the bytes, marks the stream incomplete, and
does not write them to the tee.

The probe's source writer may receive `BrokenPipe` if cancellation closes the
source reader before the writer is scheduled. The harness joins that thread,
counts this expected cleanup outcome, and still fails on other I/O errors or a
thread panic. This does not relax the valid-hit conditions.

## Local verification and scope

On Linux x86_64 with `rustc 1.94.0 (4a4ef493e 2026-03-02)`:

- `.infra/bin/align-ci.sh` completed successfully.
- `.infra/bin/ci-check.sh` completed successfully, including style, build,
  tests, documentation tests, package verification, and coverage checks.
- `cargo test --lib test_cancellation_after_read_preserves_bytes_without_starting_tee_write`
  passed after the production fix. Before the fix, the same regression failed
  because the tee received the bytes.

The Windows runner was not run locally. Per the implementation instruction,
GitHub CI ran the Windows probe and full workflow successfully on the current
task head in run 41755997380. Earlier post-fix run 41745608628 is historical
evidence for the production fix and remains separate from this worktree result.

---

# RC-002 Windows 取消交错验证

## 结果

Windows 专用探针在修复前复现了待核实交错：对阻塞 source 读取调用
`CancelSynchronousIo` 并成功返回后，worker 仍可能进入新的同步 tee 写入并阻塞。
生产代码现在会在成功读取并保留字节后、开始 tee 写入前检查取消 token。确定性回归
测试验证：已读字节仍被保留，同时不会开始 tee 写入。

修复后的 Windows 探针和完整 GitHub Actions workflow 均在提交
`cafda675f7f11cee177f76020794bf2d80a02831` 上通过：
[Rust CI run 41745608628](https://github.com/qubit-ltd/rs-command/actions/runs/41745608628)。
其中 `windows-2022`、`ubuntu-latest`、`macos-15` 验证 job 均通过，coverage、feature
matrix 和配置的 infrastructure suites 也通过。Windows 探针完成全部 128 次尝试后
通过，因此修复后没有观察到有效命中。该探针依赖竞态调度；结果是针对所测交错的证据，
不能证明所有生产调度都不可能发生类似顺序。

## 当前 worktree 探针结果

worktree 新增 `.github/workflows/windows-cancellation-interleaving.yml`：在
`windows-2022` 运行指定测试，把实际 `GITHUB_SHA` 写入 job 日志，并通过
`if: always()` 上传日志。完整 GitHub Actions run 在精确 head
`e4ca5a3c812b17b12e824b1ee71edcc03b54f7be` 上通过：
[Rust CI run 41755997380](https://github.com/qubit-ltd/rs-command/actions/runs/41755997380)。
Windows job 成功运行指定探针并输出以下分类记录。128 次 read-to-tee 尝试中，26 次
首次取消成功，102 次返回 `ERROR_NOT_FOUND`（1168），没有开始 tee 写入，也没有观察到
partial candidate。由于该探针没有证明生产 cleanup join 需要 watchdog 释放，该场景仍为
`uncovered`；RC-002 总体仍为 `unresolved`。

测试先用真实填满的匿名管道和阻塞 writer helper 验证 watchdog：独立 watchdog 线程排空
管道，并在外层进程 watchdog 超时前 join helper。结束子进程只作为外层 watchdog 的兜底。
测试输出名为 `scenario_classification` 和 `overall_classification` 的 JSON Lines 记录。
128 次尝试均无 partial candidate 时，日志包含以下记录：

```json
{"event":"scenario_classification","schema":1,"scenario":"read_to_tee","classification":"uncovered","attempts":128,"partial_probe_hits":0,"reason":"probe calls read_output_inner directly and reaps by a second cancellation plus manual pipe drain; it does not establish that production OutputReader cleanup join requires watchdog rescue"}
{"event":"scenario_classification","schema":1,"scenario":"eof_to_flush","classification":"uncovered","attempts":0,"reason":"production tee is an unbuffered File writer; no buffered bytes remain for a blocking flush after EOF. A BufferWriter/BufWriter would change the production path and cannot establish this interleaving"}
{"event":"scenario_classification","schema":1,"scenario":"stdin_check_to_write","classification":"uncovered","attempts":0,"reason":"Windows wait_writable only checks the cancellation token; there is no pending synchronous I/O in the check-to-write gap for CancelSynchronousIo to successfully cancel. A success would target the write after it entered the kernel"}
{"event":"overall_classification","schema":1,"rc002":"unresolved","reason":"all three planned scenarios lack a complete witness satisfying successful cancellation, normal completion, subsequent blocked I/O, production cleanup join, and watchdog release"}
```

read-to-tee 循环作为旧探针保留，但它直接调用 `read_output_inner`，并通过第二次
取消和手动排空管道回收；它没有证明生产 `OutputReader` 收尾 join 会一直阻塞到 watchdog
释放。生产 `File` tee writer 在 EOF 后没有待 flush 的缓冲数据；换成 `BufWriter` 会测试
不同路径。Windows 的 `PollableStdin::wait_writable` 只检查 token，因此 check-to-write
间隔没有可供成功 `CancelSynchronousIo` 命中的同步操作。这些边界分类为明确未覆盖，
而不是“无命中”。RC-002 仍未解决。

当前 Linux 本地检查：`cargo check --lib` 通过；请求的 focused 命令
`cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1
--nocapture` 退出码为 0，但因探针仅在 Windows 编译而发现 0 个测试。使用
`rustfmt --emit stdout --config skip_children=true --edition 2024` 的语法检查和
`git diff --check` 均通过。尝试 Windows 目标检查
`cargo check --tests --target x86_64-pc-windows-gnu` 时，环境缺少
`x86_64-w64-mingw32-gcc`，未能编译到本 crate。所需 Windows run 已由上文 GitHub CI 执行。

## 探针验证范围

`src/command_runner/internal/output_collector.rs` 中的 Windows 专用测试名为
`test_windows_real_cancel_read_to_blocked_tee_interleaving`。它使用真实 Windows 匿名管道、
`File` 所有的真实句柄、真实阻塞 `File::read`，以及真实的
`IoCancellation::cancel` / `CancelSynchronousIo` 调用。每次尝试前先填满 tee 管道，随后
并发放行 source 写入和取消控制线程，并记录 tee writer 在实际 `File::write` 前的入口时刻。

只有同时观察到以下情况才报告有效命中：第一次取消成功；取消返回早于 tee 写入口；
有界观察窗口结束时 worker 仍未完成；并且此时第二次真实 `CancelSynchronousIo` 也成功。
修复前的 CI run
[41740883288](https://github.com/qubit-ltd/rs-command/actions/runs/41740883288)
在第 2 次尝试中观察到一次有效命中：共 3 次尝试、3 次首次取消成功、0 次
`ERROR_NOT_FOUND`、2 次 tee 写入口和 1 次确认阻塞写。该 run 因有效命中而按设计失败，
在生产修复前保留了证据。

探针使用 `read_output_inner(..., fd=None)` 执行真实阻塞读取，因此绕过 Windows 外层的
`PeekNamedPipe` readiness 循环。结论限定于真实 read-to-tee 核心交错，并不证明完整生产
调度路径总会采用该顺序。修复后探针完成全部 128 次尝试且没有有效命中。独立的确定性
回归测试还验证了：读取后观察到取消时，已读字节仍会保留，stream 标记为未完成，且不会
写入 tee。

如果取消先关闭 source reader、source writer 后获得调度机会，写线程可能收到
`BrokenPipe`。harness 会 join 该线程并统计这种预期清理结果；其他 I/O 错误或线程 panic
仍会使测试失败。这不会放宽有效命中的判定条件。

## 本地验证与范围

在 Linux x86_64、`rustc 1.94.0 (4a4ef493e 2026-03-02)` 环境中：

- `.infra/bin/align-ci.sh` 成功。
- `.infra/bin/ci-check.sh` 成功，包含 style、build、tests、文档测试、package 验证和
  coverage 检查。
- 生产修复后，`cargo test --lib test_cancellation_after_read_preserves_bytes_without_starting_tee_write`
  通过。生产修复前，该回归测试因 tee 收到字节而失败。

按实施指示，本地没有运行 Windows runner。GitHub CI 在当前任务 head 上运行 Windows 探针，
并在 run 41755997380 中通过完整 workflow。此前的 post-fix run 41745608628 是生产修复的
历史证据，与本 worktree 的验证结果分开记录。
