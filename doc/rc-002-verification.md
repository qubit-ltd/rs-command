# REV-004 follow-up evidence (2026-11-04)

## Current implementation checkpoint

This checkpoint was initially prepared on worktree HEAD `a3207ede5f349dcd7f4638b2c4ba176bf0ed08a3` (`codex/rc002-probe-hardening-20261104`) before T3 was committed. The local host is Linux x86_64 with `rustc 1.94.0 (4a4ef493e 2026-03-02)` and `cargo 1.94.0 (85eff7c80 2026-01-15)`. `rustfmt --edition 2024 --check src/command_runner/internal/output_collector/windows_probe_tests.rs` and `git diff --check` passed. The first post-commit Windows run, GitHub Actions run `41755992521` at commit `0432781fc0774c1b54268b6eca2c07e8d5428237`, reached the Windows test build but failed with E0277 because a raw `RawHandle` pointer was captured by a worker closure and is not `Send`; none of the probes executed. The fix converts the handle address to `usize` before spawning and casts it back inside the worker. On commit `c0834701ac30d9ee8188e480901376c5da48eb02`, Windows run `41757283286` compiled and passed the watchdog test, then the readiness scenario failed because its expected `ReadinessReturned` event was treated as a premature completion; the other scenarios did not run. The controller now records that event and continues waiting for the real read entry. On final candidate commit `2dcbd43313348ca2b50749457f9b33cd03f84aa7`, Windows run `41758531911` passed the complete test suite (107 unit tests, 15 doctests) including the watchdog and all four scenario tests. The overall Windows job then failed during additional-platform Clippy because the EOF evidence collector had a collapsible nested `if`; that conditional has been collapsed. Probe tests passed, but successful per-attempt diagnostic logs were not present in this run, so counts and observed schedules remain unrecorded pending a successful project-hook run.

GitHub run `41752357431` for T1/T2 failed in both Ubuntu and Windows verification at step 9, before test execution. The log showed rustfmt checking the whole `output_collector.rs` and expecting compact formatting inherited from the prior file. It also exposed an incorrect nested module path: because the declaration is inside inline `mod tests`, the declaration now uses rustc’s nested-module path `windows_probe_tests.rs`; a tracked shim at `output_collector/tests/windows_probe_tests.rs` includes the actual sibling implementation. A minimal rustc experiment with this layout passed, and rustfmt resolves the declaration successfully. The repository alignment and CI checks were rerun successfully locally. Run `41755992521` then exposed the non-`Send` raw handle capture described above; this was fixed, and that run likewise supplies no probe result.

| Scenario | Evidence at this checkpoint | Classification |
| --- | --- | --- |
| Read-to-tee | Earlier Windows evidence below describes a real successful-cancel hit before the T1/T2 checkpoint and a post-fix 128-attempt run on an earlier commit. No new run was made at this checkpoint. The existing probe uses an anonymous pipe for the tee and calls `read_output_inner` with `fd=None`; this is not the ordinary-file tee path or full production readiness path. | Historical evidence only; current run not performed. |
| Full Windows output readiness path | Added a child scenario that supplies the real source pipe handle and total-output budget, so `wait_output_readiness` precedes the wrapped real `File::read`; the source writer is released before readiness is awaited. It records readiness return, read, tee write/flush, cancellation, cancellation flag at read return, and worker completion. | Implemented, not run on Windows at this checkpoint. |
| Stdin cancellation check-to-write | Added a child scenario that fills a real anonymous pipe, runs the production `write_stdin_until_cancelled` helper with a timestamping wrapper around the real `File::write`, and calls real `IoCancellation::cancel` after write entry. It records write count/raw OS error, cancellation flag at write return, later write entries, and worker completion. | Implemented, not run on Windows at this checkpoint. |
| EOF-to-flush | Added a child scenario that leaves the tee anonymous pipe empty and keeps the source writer open after its payload. The controller waits for the second real read entry, then synchronizes source close with the real cancellation call. It records read counts (including EOF zero), cancellation ordering, cancellation flag at read return, tee `File::flush` entry/return, and worker completion. This remains a pipe tee substitute. | Implemented, not run on Windows at this checkpoint. |

No production behavior change is justified by this checkpoint. In particular, the historical anonymous-pipe tee hit remains evidence for the already-fixed read-to-tee interleaving only; it does not establish a defect in ordinary-file tee behavior, readiness, stdin, or EOF flushing. The new scenarios have not yet been run, so their zero-hit counts and execution behavior are unknown. No zero-hit, `ERROR_NOT_FOUND` (1168), Linux result, or missing scenario is treated as proof of safety. Live Windows T3 evidence remains incomplete.

## 本轮 REV-004 补充证据（2026-11-04）

### 当前实现检查点

本检查点最初在 worktree HEAD `a3207ede5f349dcd7f4638b2c4ba176bf0ed08a3`（分支 `codex/rc002-probe-hardening-20261104`）准备，当时 T3 尚未提交。运行环境为 Linux x86_64、`rustc 1.94.0 (4a4ef493e 2026-03-02)` 和 `cargo 1.94.0 (85eff7c80 2026-01-15)`。`rustfmt --edition 2024 --check src/command_runner/internal/output_collector/windows_probe_tests.rs` 和 `git diff --check` 均通过。首次提交后的 Windows run `41755992521`（commit `0432781fc0774c1b54268b6eca2c07e8d5428237`）进入 Windows 测试编译，但因 worker closure 捕获非 `Send` 的 `RawHandle` 指针而报 E0277；探针均未执行。修复是在 spawn 前把句柄地址转成 `usize`，并在 worker 内转回指针。提交 `c0834701ac30d9ee8188e480901376c5da48eb02` 的 Windows run `41757283286` 编译通过、watchdog 测试通过，但 readiness 场景把预期的 `ReadinessReturned` 事件错当成过早完成而失败，其余场景未执行。现已修正控制器：记录该事件后继续等待真实 read 进入。必须重跑 Windows workflow 才能记录场景证据。
本检查点最初在 worktree HEAD `a3207ede5f349dcd7f4638b2c4ba176bf0ed08a3`（分支 `codex/rc002-probe-hardening-20261104`）准备，当时 T3 尚未提交。运行环境为 Linux x86_64、`rustc 1.94.0 (4a4ef493e 2026-03-02)` 和 `cargo 1.94.0 (85eff7c80 2026-01-15)`。`rustfmt --edition 2024 --check src/command_runner/internal/output_collector/windows_probe_tests.rs` 和 `git diff --check` 均通过。首次提交后的 Windows run `41755992521`（commit `0432781fc0774c1b54268b6eca2c07e8d5428237`）进入 Windows 测试编译，但因 worker closure 捕获非 `Send` 的 `RawHandle` 指针而报 E0277；探针均未执行。修复是在 spawn 前把句柄地址转成 `usize`，并在 worker 内转回指针。提交 `c0834701ac30d9ee8188e480901376c5da48eb02` 的 Windows run `41757283286` 编译通过、watchdog 测试通过，但 readiness 场景把预期的 `ReadinessReturned` 事件错当成过早完成而失败，其余场景未执行。现已修正控制器：记录该事件后继续等待真实 read 进入。最终候选提交 `2dcbd43313348ca2b50749457f9b33cd03f84aa7` 的 Windows run `41758531911` 完整测试套件通过（107 项单元测试、15 项 doctest），watchdog 和四个场景测试均通过；之后 Windows job 在额外平台 Clippy 阶段因 EOF 证据收集中的嵌套 `if` 可合并而失败。现已折叠该条件。探针测试通过，但该 run 未输出每次尝试的诊断日志，因此尝试数和观察到的交错仍待成功运行 project hook 后记录。

T1/T2 的 GitHub run `41752357431` 在 Ubuntu 和 Windows 验证 job 的 step 9 失败，测试尚未执行。日志显示 rustfmt 对整个 `output_collector.rs` 检查时，要求恢复该文件原有的紧凑格式；同时发现嵌套 module 路径错误：声明位于 inline `mod tests` 内，声明现使用 rustc 的嵌套 module 路径 `windows_probe_tests.rs`；已跟踪的 `output_collector/tests/windows_probe_tests.rs` shim 会 include 实际的同级实现文件。该布局的最小 rustc 实验通过，rustfmt 也可正确解析声明。本地重新执行仓库对齐和 CI 检查均通过。随后 run `41755992521` 暴露上述 `RawHandle` 跨线程捕获错误；现已修复，但该 run 同样没有探针结论。

| 场景 | 当前检查点的证据 | 分类 |
| --- | --- | --- |
| read-to-tee | 下文历史记录描述了 T1/T2 检查点之前真实成功取消命中，以及更早提交上的修复后 128 次尝试。本检查点没有重新运行。现有探针使用匿名管道作为 tee，并以 `fd=None` 调用 `read_output_inner`；这不是普通文件 tee 路径，也不是完整生产 readiness 路径。 | 仅有历史证据；当前未运行。 |
| 完整 Windows output readiness 路径 | 已新增子进程场景，传入真实 source pipe handle 和 total-output budget，使 `wait_output_readiness` 先于包装后的真实 `File::read` 执行；等待 readiness 前会释放 source writer。场景记录 readiness 返回、read、tee write／flush、取消、read 返回时的取消 flag 和 worker 完成。 | 已实现；本检查点尚未在 Windows 运行。 |
| stdin 取消检查到 write | 已新增子进程场景，填满真实匿名管道，通过时间戳包装器调用生产 `write_stdin_until_cancelled` 和真实 `File::write`，并在 write 进入后调用真实 `IoCancellation::cancel`。场景记录 write 字节数／原始 OS 错误、write 返回时的取消 flag、后续 write 进入和 worker 完成。 | 已实现；本检查点尚未在 Windows 运行。 |
| EOF 到 flush | 已新增子进程场景，保持 tee 匿名管道为空，source writer 写入 payload 后继续保持打开。controller 等待第二次真实 read 进入，再同步 source close 与真实取消调用。场景记录 read 字节数（含 EOF 零值）、取消顺序、read 返回时的取消 flag、tee `File::flush` 进入／返回和 worker 完成。这仍是管道 tee 替身。 | 已实现；本检查点尚未在 Windows 运行。 |

本检查点不支持生产行为修改。尤其是历史匿名管道 tee 命中只作为已修复 read-to-tee 交错的证据；不能据此确认普通文件 tee、readiness、stdin 或 EOF flush 存在缺陷。新增场景尚未运行，其零命中和执行结果均未知。零命中、`ERROR_NOT_FOUND`（1168）、Linux 结果或缺少场景均不作为安全证明。实时 Windows T3 证据仍未完成。

---

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

## Integrated Windows verification

After manually merging the task with the concurrent probe work, the full CI
workflow passed on merge commit `7fe9176d96fd183dad5890f49a3a587804170301`:
[Rust CI run 41761398112](https://github.com/qubit-ltd/rs-command/actions/runs/41761398112).
The focused workflow initially failed on that commit because it did not prepare
the repository's local `../rs-redact` path dependency (run 41762577178). The
workflow now runs `.infra/bin/prepare-local-path-dependencies.sh` before Cargo.
The rerun passed on `be0116cf12fc31602546d13226ff39f96a7251b7`:
[Windows probe run 41763162125](https://github.com/qubit-ltd/rs-command/actions/runs/41763162125).
All five tests selected by the focused filter passed. Logs show 128 attempts
with no valid read-to-tee candidate; the separate readiness and EOF-to-flush
probes also completed 128 attempts with no valid hit. The stdin scenario
observed one successful cancellation after the real write had begun and no
later write entry. These are bounded observations, not proof that every
production schedule is safe. RC-002 remains `unresolved` because the probes do
not establish that a production cleanup join requires watchdog release.

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

## 合并后的 Windows 验证

将本任务与并行 probe 工作手动合并后，完整 CI workflow 在合并提交
`7fe9176d96fd183dad5890f49a3a587804170301` 上通过：
[Rust CI run 41761398112](https://github.com/qubit-ltd/rs-command/actions/runs/41761398112)。
专用 workflow 首次在该提交上失败，因为没有准备仓库要求的本地 `../rs-redact` 路径依赖
（run 41762577178）。现已在 Cargo 前执行 `.infra/bin/prepare-local-path-dependencies.sh`。
修复后的 run 在提交 `be0116cf12fc31602546d13226ff39f96a7251b7` 上通过：
[Windows probe run 41763162125](https://github.com/qubit-ltd/rs-command/actions/runs/41763162125)。
专用筛选命中的 5 项测试全部通过。日志显示 read-to-tee 探针完成 128 次尝试且没有有效候选；
独立 readiness 和 EOF-to-flush 探针也各完成 128 次尝试且没有有效命中。stdin 场景观察到
真实 write 开始后的一次成功取消，且没有后续 write 入口。这些是有界观察，不证明所有生产
调度都安全。由于探针没有证明生产 cleanup join 需要 watchdog 释放，RC-002 仍为
`unresolved`。
