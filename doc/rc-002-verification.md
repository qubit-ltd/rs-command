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
the pushed commit triggered GitHub CI, where the Windows job and the full
workflow passed in run 41745608628.

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

按实施指示，本地没有运行 Windows runner。提交推送后由 GitHub CI 执行，Windows job 和
完整 workflow 均在 run 41745608628 中通过。
