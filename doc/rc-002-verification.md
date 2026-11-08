<a id="current-summary"></a>

## Latest evidence summary / 最新证据摘要 (2026-11-08)

The latest Windows runtime verification is at SHA `5b046b0b11ea3413403c2df060638d3cb2b2fbb7` on `windows-2022` (workflow run `42439229542`): 11 `windows_` tests, 2 command-I/O tests, and all 12 required identities passed. Its production `CommandIo` trace still lacks the required subsequent-I/O, pending-production-join, and watchdog-release causal witness, so RC-002 remains `unresolved`; the run does not establish safety. The source-only review at SHA `f9a945b9e680ab0a902921892257cc3266987af2` on Linux found no new production-reachable interleaving hypothesis. Historical Windows runs below retain their own source SHAs.

### Windows runtime verification at SHA `5b046b0b11ea3413403c2df060638d3cb2b2fbb7` (2026-11-08)

GitHub Actions workflow run `42439229542` completed on `windows-2022`. The list commands `cargo test --lib windows_ -- --list --color never` and `cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never` both exited 0. The run commands `cargo test --lib windows_ -- --show-output --test-threads=1 --color never` (11 passed) and `cargo test --lib command_io::tests::test_error_not_found_ -- --show-output --test-threads=1 --color never` (2 passed) both exited 0. The identity fixture script and result assertion passed; all 12 required test identities were discovered and passed.

The production `CommandIo` scenario recorded a successful cancellation, source read error OS code 995, cleanup join completion, and process reaping. However, `production_join_pending_witness=false`, `original_io_return_is_subsequent_io=false`, `source_writer_manually_released=true`, and `watchdog_released=false`. Across the probe scenarios there were zero confirmed blocked tee writes and zero valid hits; the EOF-to-flush probe did record successful-cancel hits, but without the required blocked-production-I/O and cleanup-join causal chain. Therefore this runtime run does not confirm the hypothesized defect or prove safety. RC-002 remains **`unresolved`**, and production behavior was not changed.

### Windows 源码 SHA `5b046b0b11ea3413403c2df060638d3cb2b2fbb7` 运行核查（2026-11-08）

GitHub Actions workflow run `42439229542` 在 `windows-2022` 完成。`cargo test --lib windows_ -- --list --color never` 和 `cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never` 两条 list 命令退出码均为 0。运行命令 `cargo test --lib windows_ -- --show-output --test-threads=1 --color never`（11 项通过）和 `cargo test --lib command_io::tests::test_error_not_found_ -- --show-output --test-threads=1 --color never`（2 项通过）退出码均为 0。身份门禁 fixtures 和结果断言通过；12 个必需测试身份均被发现且通过。

生产 `CommandIo` 场景记录到取消成功、源读取错误 OS code 995、cleanup join 完成和进程回收。但 `production_join_pending_witness=false`、`original_io_return_is_subsequent_io=false`、`source_writer_manually_released=true`、`watchdog_released=false`。各探针场景确认的阻塞 tee 写入为 0、有效命中为 0；EOF-to-flush 探针虽记录到取消成功命中，但没有形成所需的阻塞生产 I/O 与 cleanup join 因果链。因此本次运行既未证实该疑点，也未证明安全。RC-002 仍为 **`unresolved`**，生产行为未修改。

### Current task source review at SHA `f9a945b9e680ab0a902921892257cc3266987af2` (2026-11-08)

This review was performed on Linux x86_64 in the assigned implementation worktree. I reviewed the current `CommandIo` cleanup path, `read_output_inner`, the Windows cancellation token and stdin paths, both existing Windows probe implementations, and the Windows identity-gate workflow. Successful cancellation still leads to an unbounded production helper join. The output collector checks cancellation after a successful read and before tee I/O; the Windows stdin path checks cancellation before its write loop. The existing probes do not introduce a new production-reachable schedule that witnesses a later blocking production I/O, a pending `CommandIo` cleanup join caused by it, and watchdog release. Existing probe source explicitly reports `production_join_pending_witness=false`, `original_io_return_is_subsequent_io=false`, and `watchdog_released=false` for its production trace schema. Thus source review confirms the unbounded-join control-flow concern, but neither confirms a blocking later I/O nor rules out a defect. RC-002 remains **`unresolved`**; no probe or speculative production fix was added.

The host was Linux x86_64 with `rustc 1.94.0`; `pwsh` was unavailable. The Windows list/run commands and workflow identity gate were not run on this SHA, and no new runtime trace was produced. This record is source-only and is not Windows runtime or safety evidence.

### 本轮源码核查（源码 SHA `f9a945b9e680ab0a902921892257cc3266987af2`，2026-11-08）

本次在指定实施 worktree 的 Linux x86_64 环境中核查了当前 `CommandIo` cleanup 路径、`read_output_inner`、Windows 取消 token 与 stdin 路径、两个既有 Windows 探针实现，以及 Windows 身份门禁 workflow。成功取消后生产路径仍会执行无界 helper join。输出收集器在成功读取后、tee I/O 前再次检查取消；Windows stdin 路径在写循环前检查取消。既有探针没有提出新的生产可达调度，使后续阻塞生产 I/O、由其导致的 pending `CommandIo` cleanup join、以及 watchdog 释放形成见证。生产 trace schema 的探针源码明确写入 `production_join_pending_witness=false`、`original_io_return_is_subsequent_io=false`、`watchdog_released=false`。因此，源码审阅确认控制流存在无界 join，但既未确认后续 I/O 会阻塞，也未排除缺陷。RC-002 仍为 **`unresolved`**；本轮没有增加探针或猜测性生产修复。

环境为 Linux x86_64，`rustc 1.94.0`；本机无 `pwsh`。本 SHA 未运行 Windows list/run 命令或 workflow 身份门禁，也未产生新运行 trace。本记录仅为源码审阅，不构成 Windows 运行或安全证据。

### Current implementation audit at SHA `c443528ef4243fb8f056c0b4d023c68ab3d74b23` (2026-11-08)

This bounded audit was performed on Linux x86_64 after the readiness-only move. I reviewed `src/command_runner/internal/command_io.rs`, `io_cancellation.rs`, `io_cancellation_token.rs`, `output_collector.rs`, `stdin_pipe.rs`, `stdin_writer.rs`, and the existing probes in `windows_cancel_interleaving_tests.rs` and `output_collector/tests/windows_probe_tests.rs`. The cancellation sender publishes its flag with `Release`; the token observes it with `Acquire`. Output collection checks cancellation before reads and after a successful read before starting tee I/O, and flushes only after observed EOF. The Windows stdin path checks cancellation before entering the write loop. Successful cancellation still permits an unbounded helper join; the bounded confirmation wait applies after cancellation failure.

The reviewed probe sources still do not express a new production-reachable interleaving hypothesis with a subsequent production I/O, a pending `CommandIo` cleanup join caused by that I/O, and watchdog release. Existing production-probe fields explicitly report `production_join_pending_witness=false`, `original_io_return_is_subsequent_io=false`, and `watchdog_released=false`. Therefore this audit adds no runtime measurement and leaves RC-002 **unresolved**: it neither confirms nor rules out a defect, and no cancellation behavior change is justified. The host was `Linux x86_64` (`rustc 1.94.0`); `pwsh` was unavailable. The required Windows commands (`cargo test --lib windows_ -- --list --color never`, `cargo test --lib windows_ -- --show-output --test-threads=1 --color never`, and `cargo test --lib command_io::tests::test_error_not_found_ -- --show-output --test-threads=1 --color never`) and the workflow's 12-identity gate were **not run** because no Windows environment was available. This is not Windows safety evidence.

### 2026-11-08 当前实现核查（源码 SHA `c443528ef4243fb8f056c0b4d023c68ab3d74b23`）

本次有界核查在 Linux x86_64 上进行，基于 readiness 定向迁移后的源码。核对路径包括 `src/command_runner/internal/command_io.rs`、`io_cancellation.rs`、`io_cancellation_token.rs`、`output_collector.rs`、`stdin_pipe.rs`、`stdin_writer.rs`，以及既有探针 `windows_cancel_interleaving_tests.rs` 和 `output_collector/tests/windows_probe_tests.rs`。取消发送端以 `Release` 发布标志，token 以 `Acquire` 读取。输出收集在读取前及成功读取后、启动 tee I/O 前检查取消；仅观察到 EOF 后才 flush。Windows stdin 路径在进入写入循环前检查取消。成功取消后仍可能执行无界 helper join；有界确认等待仅用于取消失败的情况。

已审阅的探针源码没有提出新的生产可达交错假设，同时形成“后续生产 I/O、该 I/O 导致 `CommandIo` cleanup join 保持 pending、watchdog 释放”的因果见证。现有生产探针字段仍明确记录 `production_join_pending_witness=false`、`original_io_return_is_subsequent_io=false`、`watchdog_released=false`。因此本轮不增加运行测量，RC-002 保持 **unresolved**：既未确认也未排除缺陷，没有依据修改取消行为。执行环境为 `Linux x86_64`（`rustc 1.94.0`），本机没有 `pwsh`。计划要求的 Windows 命令（`cargo test --lib windows_ -- --list --color never`、`cargo test --lib windows_ -- --show-output --test-threads=1 --color never`、`cargo test --lib command_io::tests::test_error_not_found_ -- --show-output --test-threads=1 --color never`）及 workflow 的 12 项身份门禁均**未运行**，原因是没有可用 Windows 环境；本记录不构成 Windows 安全证据。

### Current worktree source review at SHA `1db1b9c50ee7581ff1f3758f8a2bef347f1c0275` (2026-11-07)

This review was performed in the implementation worktree on Linux x86_64 at the stated SHA; `git diff --exit-code 1db1b9c50ee7581ff1f3758f8a2bef347f1c0275 -- src/command_runner/internal/command_io.rs src/command_runner/internal/output_collector.rs src/command_runner/internal/output_reader.rs src/command_runner/internal/stdin_pipe.rs src/command_runner/internal/pollable_stdin.rs .github/workflows/windows-cancellation-interleaving.yml .github/scripts/assert-windows-probe-results.ps1 .github/scripts/assert-windows-probe-results.tests.ps1` exited 0, confirming those reviewed production and Windows workflow files match the baseline. The successful-cancellation path remains unbounded: `finish_helpers_with_wait` sends cancellation requests before helper collection, and `finish_output_reader` calls `join_output_reader` directly when cancellation succeeds. The 100 ms shared confirmation deadline applies only after cancellation failure. In the output loop, cancellation is checked before each read and again after a successful read before tee writes; flush occurs only after observed EOF. The Windows stdin path checks cancellation before allowing a write, though static inspection alone does not establish the runtime ordering of cancellation and synchronous I/O. These facts confirm the unbounded-join concern is present in control flow, but do not establish that a later production I/O blocks or holds that join pending.

The Windows workflow and PowerShell gate were reviewed at this SHA. The workflow is configured to list and execute both `windows_` and `command_io::tests::test_error_not_found_` filters, then check the 12 required full test identities across the run logs. I also read both Windows probe implementations at this baseline: `src/command_runner/internal/windows_cancel_interleaving_tests.rs` and `src/command_runner/internal/output_collector/tests/windows_probe_tests.rs`. The command `git diff --exit-code 1db1b9c50ee7581ff1f3758f8a2bef347f1c0275 -- src/command_runner/internal/windows_cancel_interleaving_tests.rs src/command_runner/internal/output_collector/tests/windows_probe_tests.rs` exited 0. Their coverage boundaries are:

| Probe path | Cancellation and subsequent I/O | Production join and watchdog evidence |
| --- | --- | --- |
| `windows_cancel_interleaving_tests.rs::run_read_to_tee_probe` | Uses a real Windows source pipe and a deliberately full pipe as tee, but calls `read_output_inner` directly. It records initial cancel success, tee-write entry, whether the worker remains unfinished, and uses a second `CancelSynchronousIo` to verify a pending tee write. | It then manually joins/releases the source writer and drains the tee before joining the worker. It does not exercise production `CommandIo` cleanup join. Its watchdog self-test proves that a separate pipe-draining thread can release a deliberately blocked pipe helper; it does not witness watchdog release of production cleanup. The source classifies read-to-tee as uncovered absent a production join witness. |
| `windows_probe_tests.rs` direct read/readiness paths | The read-to-tee and readiness scenarios likewise call `read_output_inner` with pipe-backed tee instrumentation. They can detect cancel success followed by a tee entry and a still-blocked worker; the readiness variant also observes readiness return. | These helpers recover by releasing/draining pipe paths and joining directly. They do not establish production `CommandIo` join pending. |
| `windows_probe_tests.rs` EOF-to-flush and stdin check-to-write | EOF-to-flush can record cancellation return, EOF return, and flush entry, but uses a pipe-backed tee; the source labels production EOF-to-flush uncovered because the production tee is an unbuffered `File`. The stdin probe instruments `write_stdin_until_cancelled` over a full pipe. Its event is emitted immediately before the wrapped underlying write; a successful cancellation can therefore demonstrate interruption of a pending kernel write, but not cancellation in the earlier token-check/write gap. | Neither scenario runs production `CommandIo` cleanup join. The stdin path records later write entries; it cannot establish a production join blocked by subsequent I/O. |
| `windows_probe_tests.rs::run_production_command_io_probe` | Runs `read_output_stream` and `CommandIo::cancel_and_collect` with a real source pipe and ordinary-file tee. Its event schema distinguishes the original read return (`original_io_return_is_subsequent_io=false`) and records tee writes. | The schema explicitly sets `production_join_pending_witness=false`; it samples whether cleanup is unfinished when the cancel observer fires, then manually drops the source writer. It records `watchdog_released=false` and helper joined. Thus it can verify cancellation of the original blocked read and production cleanup completion, but does not produce the required subsequent-I/O / pending-join / watchdog-release chain. |
| Probe watchdog tests | The `blocked` child is a park loop terminated by the supervisor after timeout; this checks kill/wait/reap behavior. | That is not release of a blocked production join. The separate watchdog self-test drains a deliberately full test pipe, also outside production `CommandIo`. |

These are source-level descriptions of what the probes can observe, not results from this review. The historical trace rows elsewhere in this document report their own run outcomes (including the production trace with successful cancellation, original read error 995, no subsequent I/O, no production join witness, manual source-writer release, and no watchdog release); they must not be attributed to this SHA. No current-baseline probe was executed, and this review produced no Windows run, trace, or new event evidence. `command -v pwsh` found no executable (exit code 1), so the PowerShell fixture was not run locally. No Rust tests were run; Windows-specific runtime behavior remains **unverified**, and the RC-002 classification remains **`unresolved`**. No production defect was confirmed or ruled out, so no source or probe change is supported by this review.

本次源码复核基于实施 worktree 的 Linux x86_64 环境，源码 SHA 为上述值；针对生产文件、Windows workflow 及身份脚本执行 `git diff --exit-code 1db1b9c50ee7581ff1f3758f8a2bef347f1c0275 -- src/command_runner/internal/command_io.rs src/command_runner/internal/output_collector.rs src/command_runner/internal/output_reader.rs src/command_runner/internal/stdin_pipe.rs src/command_runner/internal/pollable_stdin.rs .github/workflows/windows-cancellation-interleaving.yml .github/scripts/assert-windows-probe-results.ps1 .github/scripts/assert-windows-probe-results.tests.ps1`，退出码为 0，确认这些文件与该基线一致。成功取消路径仍会执行无界 join：`finish_helpers_with_wait` 在收集 helper 前发出取消请求；取消成功时，`finish_output_reader` 直接调用 `join_output_reader`。共享的 100 ms 确认期限只适用于取消失败的分支。输出循环在每次读取前以及成功读取后、tee 写入前检查取消；只有观察到 EOF 后才 flush。Windows stdin 路径在允许 write 前检查取消，但静态阅读不能证明取消与同步 I/O 的实际运行顺序。这些源码事实确认控制流中存在无界 join 的疑点，却不能证明后续生产 I/O 会阻塞并使该 join 挂起。

本 SHA 下复核了 Windows workflow 和 PowerShell 身份门禁：workflow 配置为分别列出并执行 `windows_` 与 `command_io::tests::test_error_not_found_` 两个过滤器，再从运行日志核验 12 个必需完整测试身份。本轮也实际阅读了两个 Windows 探针实现：`src/command_runner/internal/windows_cancel_interleaving_tests.rs` 与 `src/command_runner/internal/output_collector/tests/windows_probe_tests.rs`。命令 `git diff --exit-code 1db1b9c50ee7581ff1f3758f8a2bef347f1c0275 -- src/command_runner/internal/windows_cancel_interleaving_tests.rs src/command_runner/internal/output_collector/tests/windows_probe_tests.rs` 退出码为 0。其覆盖边界如下：

| 探针路径 | 取消与后续 I/O | 生产 join 与 watchdog 证据 |
| --- | --- | --- |
| `windows_cancel_interleaving_tests.rs::run_read_to_tee_probe` | 使用真实 Windows source pipe 和被填满的 pipe tee，但直接调用 `read_output_inner`。记录首次取消成功、tee write 入口、worker 是否仍未结束，并用第二次 `CancelSynchronousIo` 确认 tee write 是否仍为内核 pending 操作。 | 随后人工 join/release source writer、排空 tee，再 join worker；没有走生产 `CommandIo` cleanup join。watchdog 自测证明单独的排空线程能释放刻意阻塞的 pipe helper，不是生产 cleanup 被 watchdog 解救的见证。源码将 read-to-tee 标记为缺少生产 join 见证、尚未覆盖。 |
| `windows_probe_tests.rs` 的直接 read/readiness 路径 | read-to-tee 和 readiness 场景同样直接调用 `read_output_inner`，使用 pipe tee 探针；可检测取消成功后是否进入 tee write、worker 是否仍阻塞，readiness 场景还观察 readiness 返回。 | 这些 helper 通过释放／排空 pipe 后直接 join 恢复，不证明生产 `CommandIo` join pending。 |
| `windows_probe_tests.rs` 的 EOF-to-flush 与 stdin check-to-write | EOF-to-flush 可记录取消返回、EOF 返回和 flush 入口，但 tee 是 pipe；源码因生产 tee 是无缓冲 `File` 而将生产 EOF-to-flush 标记为未覆盖。stdin 探针在填满的 pipe 上探测 `write_stdin_until_cancelled`；它在包装器调用底层 write 前发出事件。成功取消可以证明一个 pending kernel write 被中断，却不能证明取消发生在更早的 token 检查与 write 调用之间。 | 两种场景都不运行生产 `CommandIo` cleanup join。stdin 会记录是否有后续 write 入口，但不能建立后续 I/O 使生产 join 阻塞的因果链。 |
| `windows_probe_tests.rs::run_production_command_io_probe` | 使用真实 source pipe、ordinary-file tee，并运行 `read_output_stream` 与 `CommandIo::cancel_and_collect`。事件格式区分原始 read 返回（`original_io_return_is_subsequent_io=false`）并记录 tee write。 | 格式明确写入 `production_join_pending_witness=false`；它采样 cancel observer 触发时 cleanup 是否未完成，随后人工 drop source writer。记录 `watchdog_released=false` 且 helper 已 join。因此可核验原始阻塞 read 被取消及生产 cleanup 最终结束，但没有形成后续 I/O／pending join／watchdog release 的所需链条。 |
| 探针 watchdog 测试 | `blocked` child 是 park loop，超时后由 supervisor 终止；验证 kill/wait/reap 处理。 | 这不是释放阻塞的生产 join。另一个 watchdog 自测排空刻意填满的测试 pipe，也处于生产 `CommandIo` 之外。 |

以上是源码审阅得到的探针观察能力，不是本轮运行结果。本文件其他位置的历史 trace 行只描述各自的旧 run（包括一条记录取消成功、原始 read 错误 995、无后续 I/O、无 production join witness、人工释放 source writer、无 watchdog release 的生产 trace），不能归属于本 SHA。本轮没有执行当前基线探针，也没有 Windows run、trace 或新事件证据。`command -v pwsh` 未找到可执行文件（退出码 1），本地未运行 PowerShell 夹具；没有运行 Rust 测试。Windows 专属运行行为仍为**未验证**，RC-002 保持 **`unresolved`**。本轮没有确认或排除生产缺陷，因此没有依据修改源码或探针。

## Implementation validation at SHA `a935e5bab3350de21002f0975c6d29867e0710e1` (2026-11-07)

The implementation checks ran on Linux x86_64. `./.infra/bin/align-ci.sh` exited 0 before implementation changes. At source SHA `a935e5bab3350de21002f0975c6d29867e0710e1`, `./.infra/bin/ci-check.sh` exited 0; build, unit/integration tests, doctests, Markdown examples, feature matrix, package verification, and security advisory scan succeeded. Coverage thresholds were met: 98.02% functions, 97.42% lines, and 97.38% regions. The standalone `./.infra/bin/coverage.sh` exited 0 and met the same thresholds. No test, CI, or coverage failure required a repair. `git diff --check` exited 0.

The focused Windows workflow `.github/workflows/windows-cancellation-interleaving.yml` was dispatched for SHA `a935e5bab3350de21002f0975c6d29867e0710e1` on `windows-2022`: [run 42392045672](https://github.com/qubit-ltd/rs-command/actions/runs/42392045672). The workflow exited successfully; PowerShell identity-gate fixtures passed, 11 `windows_` tests and 2 command-I/O tests passed, and all 12 required test identities were discovered and passed. Its production `CommandIo` trace observed successful cancellation followed by original read error 995, no subsequent I/O, no production-join-pending witness, manual source-writer release, and no watchdog release. The trace classification is `unresolved`; the successful Windows test run does not establish the suspected full causal chain.

The `rs-mime` checkout was clean on `dev-starfish` at SHA `73fe046cc52dc6c67e3a03257314456f2f14e187` and was not modified. Static review of `src/command_execution/mime_command_output.rs`, `system_mime_command_executor.rs`, and their command-execution tests confirmed that the adapter continues to preserve and check `stdout_complete` and `stdout_truncated`. Since this change makes no `qubit-command` API or behavior change, no downstream migration or `rs-mime` test run was needed.

## 2026-11-07 实施验证（源码 SHA `a935e5bab3350de21002f0975c6d29867e0710e1`）

Linux x86_64 上执行了实施检查。实施修改前运行 `./.infra/bin/align-ci.sh`，退出码为 0。源码 SHA `a935e5bab3350de21002f0975c6d29867e0710e1` 上运行 `./.infra/bin/ci-check.sh`，退出码为 0；构建、单元／集成测试、doctest、Markdown 示例、feature matrix、package 验证及安全公告扫描均成功。覆盖率门槛达成：函数 98.02%、行 97.42%、区域 97.38%。单独运行 `./.infra/bin/coverage.sh`，退出码为 0，同样达到门槛。没有测试、CI 或覆盖率失败需要修复。`git diff --check` 退出码为 0。

在 `windows-2022` 上对源码 SHA `a935e5bab3350de21002f0975c6d29867e0710e1` dispatch 了专用 workflow `.github/workflows/windows-cancellation-interleaving.yml`：[run 42392045672](https://github.com/qubit-ltd/rs-command/actions/runs/42392045672)。workflow 成功退出；PowerShell 身份门禁夹具通过，11 项 `windows_` 测试及 2 项 command-I/O 测试通过，12 个必需测试身份全部发现并通过。生产 `CommandIo` trace 观察到取消成功后原始 read 返回错误 995，没有后续 I/O、production join pending 见证或 watchdog 释放；source writer 是人工释放的。该 trace 分类仍为 `unresolved`；Windows 测试成功并不构成完整疑点因果链的证明。

`rs-mime` 仓库在 `dev-starfish` 分支、SHA `73fe046cc52dc6c67e3a03257314456f2f14e187`，工作树干净，本轮没有修改。静态核对 `src/command_execution/mime_command_output.rs`、`system_mime_command_executor.rs` 及对应 command execution 测试，确认适配器继续保留并检查 `stdout_complete` 与 `stdout_truncated`。本轮没有改变 `qubit-command` API 或行为，因此无需下游迁移或运行 `rs-mime` 测试。

### Source review at SHA `1b6da900e7a5f25384116efae4b26d22f9944325` (2026-11-06)

At checkout SHA `1b6da900e7a5f25384116efae4b26d22f9944325`, a read-only review covered `src/command_runner/internal/command_io.rs`, `src/command_runner/internal/output_collector.rs`, `src/command_runner/internal/pollable_stdin.rs`, and the Windows evidence summarized in this file. `command_io` issues cancellation requests before waiting for helpers; successful cancellation proceeds directly to an unbounded join, while failed cancellation gets a shared bounded completion-confirmation wait and may detach if still unconfirmed. `output_collector` checks cancellation before reading and again after a successful read before starting tee I/O; it flushes only after observed EOF. On Windows, `pollable_stdin` checks cancellation before permitting the next synchronous write. No new production-reachable interleaving assumption or new causal witness was found. Windows was not run and no trace was produced at this SHA. The previously recorded production trace has successful cancellation but an OS error 995 on the original read, no subsequent production I/O, no pending production join witness, and no watchdog release. Classification remains **`unresolved`**; no production fix is supported by this review.

### SHA `1b6da900e7a5f25384116efae4b26d22f9944325` 源码复核（2026-11-06）

在 checkout SHA `1b6da900e7a5f25384116efae4b26d22f9944325` 上，只读核查了 `src/command_runner/internal/command_io.rs`、`src/command_runner/internal/output_collector.rs`、`src/command_runner/internal/pollable_stdin.rs` 及本文记录的 Windows 证据。`command_io` 先发出所有取消请求，再等待 helper；成功取消后直接执行无界 join，取消失败时则使用共享的有界完成确认等待，仍未确认时可以 detach。`output_collector` 在读取前检查取消，并在成功读取后、开始 tee I/O 前再次检查；只有观察到 EOF 后才 flush。Windows 下的 `pollable_stdin` 在允许下一次同步 write 前检查取消。本轮未发现新的生产可达交错假设或因果见证。本 SHA 未运行 Windows，也未产生 trace。既有生产 trace 记录了取消成功，但原 read 返回 OS 错误 995，没有后续生产 I/O、pending production join 见证或 watchdog 释放。结论仍为 **`unresolved`**；本轮核查不支持生产修复。

| Evidence / 证据 | Source SHA / 源码 SHA | Platform / 平台 | Command / 命令 | Selected tests / 选中测试 | Run and artifact / 运行与工件 | Witness fields / 见证字段 | Conclusion / 结论 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Source review / 源码复核 (2026-11-06) | `1b6da900e7a5f25384116efae4b26d22f9944325` | Linux x86_64 workspace | Read-only `git rev-parse HEAD`, `rg -n`, and targeted `sed -n` inspection of `src/command_runner/internal/command_io.rs`, `output_collector.rs`, and `pollable_stdin.rs` | N/A (no tests run / 未运行测试) | No Windows run, trace, or artifact for this review / 本次复核未运行 Windows、未产生 trace 或 artifact | `command_io`: cancellation requests are batched before helper waits; a failed cancellation is bounded by completion confirmation before join/detach. `output_collector`: cancellation is checked before tee I/O; flush follows observed EOF. Windows `pollable_stdin`: cancellation is checked immediately before permitting the next write. No new reachable interleaving assumption found. | `unresolved`: source inspection only; neither proves the suspected defect nor establishes safety. |
| Bounded source audit / 有界源码审查 (2026-11-07) | `9304afb53679f11f3707753d9b9a396baa78d448` | Linux x86_64 workspace | Read-only inspection of `command_io.rs`, `io_cancellation.rs`, `cancel.rs`, `output_collector.rs`, `pollable_stdin.rs`, and `windows_cancel_interleaving_tests.rs` | N/A (no tests run / 未运行测试) | No Windows run or trace / 未运行 Windows、未产生 trace | No new production-reachable interleaving hypothesis or causal witness found. | `unresolved`: bounded source review only; Windows behavior remains unverified and safety is not established. |
| Windows identity gate / Windows 身份门禁 (2026-11-06) | `dde6481b0f00e3abed66a3a7dc379f3d4c9229f9` | `windows-2022` | `cargo test --lib windows_ -- --list --color never`; `cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never`; both corresponding `--show-output --test-threads=1 --color never` runs | 13 selected across two filters; 12 required identities discovered and passed | [run 42191457990](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990); [artifact 13490440897](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990/artifacts/13490440897) | PowerShell fixtures passed; both Cargo list/run exit codes were 0; required identity summary reports 12/12 discovered and passed | Identity coverage verified; this does not add a production causal witness or change `unresolved`. |
| Latest Windows run / 最新 Windows 运行 (2026-11-07) | `2506ff496f54de1b991ea0ed161f9bf66556327e` | `windows-2022` | `cargo test --lib windows_ -- --list --color never`; `cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never`; corresponding `--show-output --test-threads=1 --color never` runs | 11 Windows + 2 command-I/O tests passed; all 12 required identities discovered and passed | [run 42349303349](https://github.com/qubit-ltd/rs-command/actions/runs/42349303349); [artifact 13565042213](https://github.com/qubit-ltd/rs-command/actions/runs/42349303349/artifacts/13565042213) | Production read-to-tee: cancel succeeded; original read returned OS error 995; `original_io_return_is_subsequent_io=false`; `production_join_pending_witness=false`; `source_writer_manually_released=true`; `watchdog_released=false`; helper joined and process was reaped. | `unresolved`: the latest trace still lacks subsequent blocked production I/O, a pending production join, and watchdog release. |
| Current source/history review / 本轮源码与历史复核 (2026-11-06) | `49b1b58912dc551832f36edaa80f123b976b4707` | Linux x86_64 workspace | Read-only `git rev-parse HEAD`, `rg -n`, and targeted `sed -n` inspection | N/A (no tests run / 未运行测试) | No run or artifact / 无 run 或 artifact | Historical witness fields remain as listed above; no new trace was produced / 历史见证字段同上；本轮无新 trace | `unresolved`: no new production interleaving assumption was found; this review neither proves the defect nor proves safety. |
| Implementation Windows run / 实施阶段 Windows 运行 (2026-11-06) | `146503392975780fa490afd5f7d3d46248a73cee` | `windows-2022` | `cargo test --lib windows_ -- --list --color never`; `cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never`; then both filters with `--show-output --test-threads=1 --color never` | 11 `windows_` tests + 2 command I/O tests passed; all 12 required identities discovered and passed | [run 42225665684](https://github.com/qubit-ltd/rs-command/actions/runs/42225665684); [artifact 13506405239](https://github.com/qubit-ltd/rs-command/actions/runs/42225665684/artifacts/13506405239) | One production trace: cancel succeeded; original read returned OS error 995; `original_io_return_is_subsequent_io=false`; `production_join_pending_witness=false`; `source_writer_manually_released=true`; `watchdog_released=false`; helper joined and process was reaped | `unresolved`: Windows identities and probes ran, but the trace still lacks subsequent blocked production I/O, pending production join, and watchdog release. |

本摘要分别记录 SHA `9304afb53679f11f3707753d9b9a396baa78d448` 的有界审查和 SHA `1db1b9c50ee7581ff1f3758f8a2bef347f1c0275` 的源码复核；两者都未运行 Windows。最新实施验证基于 SHA `a935e5bab3350de21002f0975c6d29867e0710e1`，其 Windows workflow 在 `windows-2022` 成功，11 项 `windows_` 测试、2 项 command-I/O 测试及全部 12 个必需身份均通过。生产 trace 仍缺少后续 I/O、production join pending 和 watchdog 释放见证，因此 RC-002 仍为 `unresolved`；该次运行不能证明安全。下方历史 Windows 运行保留各自源码 SHA。

| 证据 | 源码 SHA | 平台 | 命令 | 选中测试 | Run 与 artifact | 见证字段 | 结论 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 源码复核（2026-11-06） | `1b6da900e7a5f25384116efae4b26d22f9944325` | Linux x86_64 workspace | 只读执行 `git rev-parse HEAD`、`rg -n` 和定点 `sed -n` 检查 `src/command_runner/internal/command_io.rs`、`output_collector.rs`、`pollable_stdin.rs` | 不适用（未运行测试） | 本次复核没有 Windows 运行、trace 或 artifact | `command_io`：先批量请求取消，再等待 helper；取消失败时先限时确认完成，再 join/detach。`output_collector`：tee I/O 前检查取消；只有观察到 EOF 后才 flush。Windows `pollable_stdin`：允许下一次 write 前检查取消。没有发现新的可达交错假设。 | `unresolved`：仅源码检查；既不证明疑似缺陷，也不证明安全。 |
| 有界源码审查（2026-11-07） | `9304afb53679f11f3707753d9b9a396baa78d448` | Linux x86_64 workspace | 只读检查 `command_io.rs`、`io_cancellation.rs`、`cancel.rs`、`output_collector.rs`、`pollable_stdin.rs` 和 `windows_cancel_interleaving_tests.rs` | 不适用（未运行测试） | 未运行 Windows、未产生 trace | 未发现新的生产可达交错假设或因果见证。 | `unresolved`：仅有界源码审查；Windows 行为仍未验证，不能据此认定安全。 |
| Windows 身份门禁（2026-11-06） | `dde6481b0f00e3abed66a3a7dc379f3d4c9229f9` | `windows-2022` | `cargo test --lib windows_ -- --list --color never`；`cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never`；两条对应的 `--show-output --test-threads=1 --color never` 运行 | 两种过滤器共选中 13 项；12 个必需身份均已发现并通过 | [run 42191457990](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990)；[artifact 13490440897](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990/artifacts/13490440897) | PowerShell 夹具通过；两组 Cargo list/run 退出码均为 0；身份摘要显示 12/12 均发现并通过 | 身份覆盖已验证；本轮没有新增生产因果见证，分类仍为 `unresolved`。 |
| 最新 Windows 运行（2026-11-07） | `2506ff496f54de1b991ea0ed161f9bf66556327e` | `windows-2022` | `cargo test --lib windows_ -- --list --color never`；`cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never`；随后对两个 filter 执行 `--show-output --test-threads=1 --color never` | Windows 11 项、command I/O 2 项通过；12 个必需身份全部发现并通过 | [run 42349303349](https://github.com/qubit-ltd/rs-command/actions/runs/42349303349)；[artifact 13565042213](https://github.com/qubit-ltd/rs-command/actions/runs/42349303349/artifacts/13565042213) | 生产 read-to-tee：取消成功；原始 read 返回 OS 错误 995；`original_io_return_is_subsequent_io=false`；`production_join_pending_witness=false`；`source_writer_manually_released=true`；`watchdog_released=false`；helper 已 join、进程已回收 | `unresolved`：最新 trace 仍缺少后续阻塞生产 I/O、pending production join 和 watchdog 释放。 |
| 本轮源码与历史复核（2026-11-06） | `49b1b58912dc551832f36edaa80f123b976b4707` | Linux x86_64 workspace | 只读执行 `git rev-parse HEAD`、`rg -n` 和定点 `sed -n` 检查 | 不适用（未运行测试） | 无 run 或 artifact | 历史见证字段同上；本轮没有新 trace | `unresolved`：没有发现新的生产交错假设；本轮既不证明缺陷，也不证明安全。 |
| 实施阶段 Windows 运行（2026-11-06） | `146503392975780fa490afd5f7d3d46248a73cee` | `windows-2022` | `cargo test --lib windows_ -- --list --color never`；`cargo test --lib command_io::tests::test_error_not_found_ -- --list --color never`；随后对两个 filter 执行 `--show-output --test-threads=1 --color never` | `windows_` 11 项、command I/O 2 项通过；12 个必需身份全部发现并通过 | [run 42225665684](https://github.com/qubit-ltd/rs-command/actions/runs/42225665684)；[artifact 13506405239](https://github.com/qubit-ltd/rs-command/actions/runs/42225665684/artifacts/13506405239) | 一次生产 trace：取消成功；原始 read 返回 OS 错误 995；`original_io_return_is_subsequent_io=false`；`production_join_pending_witness=false`；`source_writer_manually_released=true`；`watchdog_released=false`；helper 已 join、进程已回收 | `unresolved`：Windows 身份和探针已运行，但 trace 仍缺少后续阻塞生产 I/O、production join pending 和 watchdog 释放。 |

## Automatic Windows identity gate and RC-002 evidence (2026-11-06)

The `push` event automatically ran the Windows cancellation workflow on source SHA
`5024c338355cbea04e98ae5cffd5e3e7b2435fe6` on `windows-2022`. The PowerShell
identity-gate fixtures passed. The workflow listed and ran both `windows_` and
`command_io::tests::test_error_not_found_` filters; 11 Windows tests and 2
command-I/O tests passed. All 12 required full test identities were discovered
and passed. The workflow completed successfully: [run 42251303125](https://github.com/qubit-ltd/rs-command/actions/runs/42251303125),
[diagnostic artifact 13521056393](https://github.com/qubit-ltd/rs-command/actions/runs/42251303125/artifacts/13521056393).

The production `CommandIo` trace at that SHA observed successful cancellation,
but the original read returned Windows error 995. It recorded zero ordinary-file
tee writes, `original_io_return_is_subsequent_io=false`,
`production_join_pending_witness=false`, `source_writer_manually_released=true`,
and `watchdog_released=false`; the helper joined and child was reaped. This
automatic run verifies the identity gate and exercises the existing probe, but
does not establish the suspected subsequent-blocked-I/O/pending-join causal
chain. RC-002 remains **`unresolved`**; no production behavior change is
supported by this trace.

## 自动 Windows 身份门禁与 RC-002 证据（2026-11-06）

Windows cancellation workflow 由 `push` 事件自动触发，在 `windows-2022` 对源码 SHA
`5024c338355cbea04e98ae5cffd5e3e7b2435fe6` 运行。PowerShell 身份门禁夹具通过。
workflow 分别发现并运行 `windows_` 与
`command_io::tests::test_error_not_found_` 两种 filter；11 项 Windows 测试和 2 项
command-I/O 测试通过。12 个必需完整测试身份全部发现并通过。workflow 成功完成：
[run 42251303125](https://github.com/qubit-ltd/rs-command/actions/runs/42251303125)，
[诊断 artifact 13521056393](https://github.com/qubit-ltd/rs-command/actions/runs/42251303125/artifacts/13521056393)。

该 SHA 的生产 `CommandIo` trace 观察到取消成功，但原 read 返回 Windows 错误 995。
trace 记录 ordinary-file tee 写入为 0，`original_io_return_is_subsequent_io=false`、
`production_join_pending_witness=false`、`source_writer_manually_released=true`、
`watchdog_released=false`；helper 已 join，child 已回收。本次自动运行验证了身份门禁并执行了
现有探针，但没有建立疑似的“后续阻塞 I/O／pending join”因果链。RC-002 仍为
**`unresolved`**；该 trace 不支持修改生产行为。

## RSC-002 Windows test identity gate verification (2026-11-06)

The Windows workflow passed on `windows-2022` at source SHA
`dde6481b0f00e3abed66a3a7dc379f3d4c9229f9`. It ran PowerShell gate fixtures, then
discovered and executed the `windows_` filter and the separately selected
`command_io::tests::test_error_not_found_` tests. Both list commands and both run
commands exited 0; 13 tests were selected across the two run commands. The result
gate reported all 12 required full test identities as discovered and passed.
See [workflow run 42191457990](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990)
and [artifact 13490440897](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990/artifacts/13490440897).

This verifies that the required test identities are selected and successful. The
probe trace remains bounded historical evidence and does not establish a new
production cancellation/join causal witness; the existing `unresolved`
classification is unchanged.

## RSC-002 Windows 测试身份门禁验证（2026-11-06）

Windows workflow 在 `windows-2022`、源码 SHA
`dde6481b0f00e3abed66a3a7dc379f3d4c9229f9` 上通过。它先运行 PowerShell
门禁夹具，再发现并执行 `windows_` 过滤器以及单独选择的
`command_io::tests::test_error_not_found_` 测试。两个 list 命令和两个运行命令的
退出码均为 0，两条运行命令共选择 13 项。结果门禁报告 12 个必需完整测试身份均已发现并通过。
详见 [workflow run 42191457990](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990)
和 [artifact 13490440897](https://github.com/qubit-ltd/rs-command/actions/runs/42191457990/artifacts/13490440897)。

该验证确认必需测试身份被选中并成功执行。探针 trace 仍是有界历史证据，没有建立新的生产取消/join
因果见证；现有 `unresolved` 分类保持不变。

## WIN-CANCEL-JOIN 源码与历史证据复核（2026-11-06）

本轮仅复核源码与本文件已有历史记录，未运行测试或 Windows 探针。检查的工作树源码 SHA 为
`49b1b58912dc551832f36edaa80f123b976b4707`（Linux workspace；无本轮 Windows 运行命令或选中测试数）。
核查范围包括 `command_io.rs`、`output_collector.rs`、`output_reader.rs`、`stdin_pipe.rs`、
`pollable_stdin.rs`、`io_cancellation.rs`、`windows_cancel_interleaving_tests.rs`、
`output_collector/tests/windows_probe_tests.rs`、`.github/workflows/windows-cancellation-interleaving.yml`
以及本证据文件内的 RSC-003、CMD-REVIEW-003 和 CMD-REASSESS-002 记录。历史 run、artifact 与源码 SHA
仅按本地记录核对，本轮未独立查询远端。

### 因果轨迹核对

| 事件 | 核查结论 | 证据边界 |
| --- | --- | --- |
| 成功取消 | Windows `IoCancellation::cancel` 先设置取消 token，再对 helper 调用 `CancelSynchronousIo`。既有生产 trace 记录其成功取消正在阻塞的原始 pipe read。 | 该运行属于其记录的历史 SHA，不是本轮源码 SHA。 |
| 原操作返回 | 生产 trace 中被取消的原始 read 返回 Windows 错误 995（`ERROR_OPERATION_ABORTED`），不是原操作正常完成。 | 原始 read 的返回不能算作后续 I/O。 |
| 后续真实阻塞 I/O | `read_output_inner` 捕获成功读取的字节后检查 token，再开始同步 tee 写；取消仍可能与检查和 tee 写入口竞态，但本轮没有发现超出已记录窗口的新可达路径假设。既有生产 trace 的 ordinary-file tee 写入数为 0。 | 源码上的竞态窗口不证明它在该轨迹中发生或阻塞。read-to-tee 的 128 次历史探针直接调用内层函数，不能替代完整生产 reader/join 轨迹。 |
| 生产 join pending | `finish_helpers` 在取消所有 helper 后收尾；取消成功时 `finish_output_reader` 直接调用 `join_output_reader`。共享确认期限只用于取消失败且需确认 helper 完成的分支，不限制成功取消后的 join。 | 这证明成功取消后 join 没有此处的期限，不证明 join 因后续 I/O 实际挂起。生产 trace 的 `production_join_pending_witness=false`；observer 时刻的 cleanup 未完成仅是采样状态。 |
| watchdog 解救 | 生产 trace 记录 `watchdog_released=false`，source writer 是手动释放的。 | 独立 watchdog 自测释放刻意阻塞的真实 pipe，只证明 harness watchdog 自身可用，不能并入生产轨迹。 |
| helper/process 回收 | 既有生产 trace 最终记录 helper join 和 child reap。 | 回收发生在未观察到生产 join pending、且 watchdog 未释放的轨迹中，不能补足缺失因果链。 |

read-to-tee、EOF-to-flush、stdin check-to-write 等历史探针结果仍按各自记录解释：pipe/缓冲 writer 替代路径不是生产 ordinary-file tee；stdin 的真实内核 write 取消不是 check-to-write 间隙内的取消；有限尝试未命中也不构成安全证明。不同 run 的成功取消、watchdog 自测和回收事件不能拼成一条生产见证。

### 结论与本轮范围

完整待证链仍为：成功取消某操作、该原操作正常返回、随后进入真实阻塞的生产同步 I/O、生产 cleanup join 因此 pending，最后由 watchdog 释放并回收 helper/process。现有记录没有建立该同轨迹链；也没有证明生产路径安全。本轮结论继续为 **`unresolved`**，没有新的生产路径假设支持修复，因此未改生产源码、Windows 探针或取消策略，也未重复运行既有 128 次探针。

本轮可复核的操作是只读源码／文档检查（包括 `git -C <worktree> rev-parse HEAD`、针对上述符号和证据字段的 `rg -n`、以及定点 `sed -n` 查看）；没有运行 `cargo test`，没有 Windows 环境结果，也没有新增实验或新的 Windows 证据。后续摘要应把本节作为 2026-11-06 源码/历史证据复核，不应改写为本轮 Windows 验证。

## RSC-004 bounded harness adjustments (2026-11-05)

> Archived Windows runtime evidence referenced by the [latest evidence summary](#current-summary); this is the latest existing Windows run, not a run from this implementation round.

This implementation pass reviewed the existing Windows probes and production path
at source base `f9bc76fa98f1437647cc773cc6bce9e0fb1f2b45`. The post-integration
Windows run [42015431077](https://github.com/qubit-ltd/rs-command/actions/runs/42015431077)
ran on `windows-2022` at source SHA `07c7d76a1ba16a81085a09f9cdd1b557c86ec579`.
The exact command was `cargo test --lib windows_ -- --nocapture`; all 11 selected
tests passed (0 failed, 109 filtered). Its [structured trace artifact](https://github.com/qubit-ltd/rs-command/actions/runs/42015431077/artifacts/13408423993)
records one production `CommandIo` attempt: cancellation succeeded, the original
blocked read returned Windows error 995, the cleanup thread was unfinished when the
cancel observer fired, and the reader helper later joined. However,
`production_join_pending_witness=false`, `original_io_return_is_subsequent_io=false`,
`source_writer_manually_released=true`, and `watchdog_released=false`. Thus it still
does not establish the complete causal witness; cancellation policy remains unchanged
and unresolved.

The existing probes were retained. Their scenario limit remains 128 attempts;
independent probe subprocess watchdogs are now 10 seconds. The workflow gives the
probe step a 15-minute outer timeout and the Windows job a 20-minute timeout. The
ordinary-file production tee and EOF-with-no-pending-buffer classification remain
unchanged; no synthetic writer or I/O was introduced.

The run exercised the independent child watchdog and confirmed it released and
reaped the deliberately blocked child. The readiness scenario recorded 128 attempts,
128 successful cancellations, 128 tee-write entries, and zero confirmed blocked
writes. The production scenario remains a single bounded attempt because the missing
events are causal witnesses, not merely low sample counts. The RSC-004 conclusion is
unresolved: this evidence neither confirms the suspected production hang nor proves
that it cannot occur. Linux zero-test discovery is not treated as Windows evidence.

# Archived summary: Current status / 归档摘要：当前状态 (2026-11-04)

> Archived historical summary. See the [latest evidence summary](#current-summary); this table is retained for its historical item mapping.

This is the archived 2026-11-04 summary for CMD-REV-001 through CMD-REV-004 and CMD-REASSESS-002. It preserves the item mapping and evidence as recorded then; see the [latest evidence summary](#current-summary) for the current status.

| Review item | Current status and evidence | Gap / historical context |
| --- | --- | --- |
| CMD-REV-001 — make marked examples self-contained | Verified on Linux x86_64 at implementation tree SHA `3d8461a` (based on checkout `7b2a0adfce45db127ea137ea84268b95d239d5c2`). `CARGO_TARGET_DIR=/tmp/rs-command-target-t1-focused cargo test --test documentation_examples_tests` passed: 4 harness tests; 12 generated/selected examples, 6 Unix examples passed and 6 `compile`/`no_run` examples were type-checked and ignored at runtime. Before the harness fix, removing the import from either external README exited 0 because the injected glob import masked the omission; after the fix, each removal failed to compile with E0433 for `CommandRunner` and `Command`. | Windows-specific examples are excluded by `cfg(windows)` on Linux, so they were not compiled or run here. See [current local implementation verification](#current-local-implementation-and-verification-2026-11-04). |
| CMD-REV-002 — organize current status and historical evidence | At that stage, the status table and archive/context labels documented the then-current evidence; they did not create new runtime evidence. | Older labels CMD-R02, CMD-003, RC-001, and RC-002 came from different review stages and are not proven one-to-one equivalents. See [context and archive index](#archived-evidence-index). |
| CMD-REV-003 — document readiness variants and fields | Completed in the 2026-11-04 task worktree; its source rustdoc is summarized in [that implementation and verification record](#current-local-implementation-and-verification-2026-11-04). | No platform behavior change is claimed. Historical RC-002 naming is contextual only. |
| CMD-REV-004 — verify the Windows cancellation/join concern first | **Bounded verification completed; behavior unresolved and no production policy change justified.** Windows run [41830652995](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995) tested exact task SHA `4812685a10616c663ab207351c9812356da36b21` on `windows-2022`: 6 selected, 6 passed, 0 failed, 105 filtered; [trace artifact 13322432766](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995/artifacts/13322432766). The production scenario did not establish the required causal witness. | Trace observed cancellation success and original read error 995, but `production_join_pending_witness=false`; the original read return was not subsequent I/O, source writer was manually released, and watchdog did not release it. The helper joined and child was reaped. See [current implementation and verification](#current-local-implementation-and-verification-2026-11-04) and [archived RC-002 results](#rc-002-windows-cancellation-interleaving-verification). |
| CMD-REASSESS-002 — reassess Windows successful-cancel/production-join concern | **Fresh bounded Windows run completed; classification remains `unresolved`; no production change justified.** Run [41851372381](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381), `windows-2022`, exact source SHA `70c343d155c88852443ae327e37baded29f318d4`: command selected 6 tests, all passed, 0 failed, 110 filtered; [artifact 13335311428](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381/artifacts/13335311428). | The production attempt did not establish subsequent blocked I/O or a pending production join; see [fresh CMD-REASSESS-002 evidence](#cmd-reassess-002-fresh-windows-evidence-2026-11-04). This row is separate from the historical CMD-REV-004 row. |

## CMD-REVIEW-003 evidence review (2026-11-05)

> Archived source review. See the [latest evidence summary](#current-summary) for the current source-review result.

This is a source-path review only; no Windows experiment was run in this Linux session. The assigned worktree HEAD is `b098a4c6d1b383bc8a4d5302884913e465643be2`. At inspection, the worktree also had concurrent in-progress edits in `src/command_cancellation.rs`, `src/command_runner/internal/output_collector.rs`, and `src/command_runner/internal/total_output_budget.rs`; these were preserved. The inspected changes in those files are Rustdoc edits and do not alter the cancellation/read/join behavior described below. The pre-existing Windows trace evidence remains tied to its recorded SHAs and runs above, not to this worktree HEAD.

The relevant production path is `CommandIo::cancel_and_collect` → `finish_helpers` → `OutputReader::cancel` → `finish_output_reader` → `join_output_reader`. `finish_helpers` requests cancellation for stdout, stderr, and stdin before finishing any helper. If cancellation succeeds, `finish_output_reader` proceeds directly to `join_output_reader`; its confirmation deadline only bounds waiting after a cancellation failure, not the join after success. Thus a genuinely blocked later operation could leave production cleanup waiting in the join, but the source path alone does not prove that schedule occurs.

`read_output_inner` checks the cancellation token before each read. After a successful read it captures the bytes, checks cancellation again, and exits before starting tee I/O if the token is already set. Cancellation can race after the loop check or Windows readiness check and before the synchronous pipe read; it can also race after the post-read check and before an ordinary-file tee write. After a successful tee write, the next loop iteration checks the token before another pipe read. The production tee is an unbuffered ordinary `File`, so the historical buffered/pipe flush probes do not represent its path. Windows stdin similarly checks cancellation in `wait_writable` immediately before the synchronous write; there is no pending operation in that check-to-write gap. A write that has already entered the kernel is a different event.

The historical production trace observed successful native cancellation of the **original blocked read**, its return with raw Windows error 995, and eventual helper join/process reap. It recorded `original_io_return_is_subsequent_io=false`, `production_join_pending_witness=false`, `source_writer_manually_released=true`, and `watchdog_released=false`. “Cleanup unfinished when the cancellation observer fired” is only a timing sample; it does not establish a production join pending behind a subsequent blocked operation. The standalone watchdog self-test establishes that the harness watchdog can release its deliberately blocked pipe and reap its helper/child, not that watchdog rescue occurred in the production scenario.

No new experiment was run. Existing evidence already includes the bounded 128-attempt read-to-tee, readiness, and EOF-to-flush probes plus the coordinated production blocked-read trace. Repeating them without an additional event observation or a production-faithful way to hold and identify a later synchronous operation would only add another sample of the same incomplete chain. A test-only pause or a pipe/`BufWriter` substitute could manufacture a schedule, but would not establish that ordinary-file tee or pipe handling in the production path blocks after successful cancellation.

The missing causal witness remains: successful cancellation associated with an operation, normal completion of that original operation, entry into a **subsequent** blocked synchronous operation, production cleanup demonstrably pending in its join, and watchdog release followed by helper/process reclamation. The prior traces do not establish all of those events, so CMD-REVIEW-003 remains **unresolved**—neither confirmed, disproven, nor fixed. No production or test changes were made for this review. No Windows-only test was executed on Linux, and no Linux zero-test result is represented as Windows evidence.

## CMD-REASSESS-002 fresh Windows evidence (2026-11-04)

> Archived Windows run. See the [latest evidence summary](#current-summary) for the newest available Windows evidence.

A fresh run of the existing Windows workflow tested source SHA `70c343d155c88852443ae327e37baded29f318d4` on `windows-2022`: [run 41851372381](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381), [artifact 13335311428](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381/artifacts/13335311428). Command: `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`; 6 selected, 6 passed, 0 failed, 110 filtered. The artifact contains the complete trace; its SHA-256 is `66032f5773f2756d954f5e18dcfd17ae2eeae1284ad1af13e9400dbdcc8ff048`.

The production `CommandIo` blocked-read scenario ran once. The observed order was read entry at 7.1102 ms, cleanup start at 17.3933 ms, successful cancellation return at 17.7285 ms, original read return at 17.7461 ms with raw OS error 995, then cleanup join at 17.9244 ms. At the cancellation observer, cleanup was unfinished, but `production_join_pending_witness=false`; `original_io_return_is_subsequent_io=false`. The path used an ordinary-file tee, recorded zero tee writes, and never entered a later I/O. The helper ultimately joined and the process was reaped; the source writer was manually released, while the watchdog did not release it. This does not establish the production pending-join/rescue chain.

Bounded probe results: read-to-tee ran 128 attempts (30 successful cancellation calls, 98 error 1168, 0 tee-write entries, 0 confirmed blocked writes, 0 valid partial hits). It calls `read_output_inner` directly and reaps via a second cancellation plus manual pipe drain, so it does not test production `OutputReader` cleanup join. Readiness ran 128 attempts (128 successful cancellation calls, 128 tee-write entries, 0 confirmed blocked writes, 0 valid hits); it did not establish subsequent blocked I/O or a pending production join. EOF-to-flush ran 128 pipe-tee substitute attempts (45 successful cancellations, 83 errors 1168, 0 confirmed blocked writes); the production case is inapplicable because production tee is an unbuffered ordinary `File`, with no pending buffered bytes to block a post-EOF flush. The stdin check-to-write gap had 0 attempts because Windows `wait_writable` only checks cancellation before the synchronous write; its separate one real in-kernel write cancellation succeeded and returned 995, which does not witness that gap. Watchdog self-test verified a blocked real pipe, watchdog release, helper reaping, and child reaping without child kill.

The overall classification is **`unresolved`**: no trace satisfies the full witness sequence of successful cancellation, normal return of that operation, a later blocked I/O, pending production cleanup join, and watchdog release/recovery. The event observer and existing synchronization points were reused; no production writer or I/O was changed. No second experimental round was warranted: current evidence locates the missing causal events, and another round using a buffered/pipe tee or injecting a post-cancel operation would manufacture operations absent from the production path rather than improve evidence about it. Existing historical 128-attempt scenarios already provide bounded coverage. No Linux `cfg` run is represented as Windows verification, and no production, test, or other documentation files were changed for this item.

## CMD-AUD-003 Windows verification at final implementation SHA

The final implementation tree at `32488efaa2f1e0e8b2d074e97d9667b6fbdadfc7` was checked by the existing `windows-cancellation-interleaving.yml` workflow on `windows-2022`: [run 41844950745](https://github.com/qubit-ltd/rs-command/actions/runs/41844950745), [trace artifact 13331255662](https://github.com/qubit-ltd/rs-command/actions/runs/41844950745/artifacts/13331255662). It ran `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`: 6 passed, 0 failed, 110 filtered. The full three-platform workflow [41844961106](https://github.com/qubit-ltd/rs-command/actions/runs/41844961106) also passed at the same source SHA; its Windows job passed `test_windows_probe_watchdog_reaps_blocked_child` (1 passed, 115 filtered).

The production `CommandIo` blocked-read case ran once. Cancellation succeeded; the original read returned Windows error 995; the reader helper joined and the process was reaped. The trace recorded `production_join_pending_witness=false`, `original_io_return_is_subsequent_io=false`, `source_writer_manually_released=true`, and `watchdog_released=false`, so it does not establish the required production cleanup join hang/rescue chain. Bounded substitute probes recorded: read-to-tee 128 attempts (45 successful first cancels, 83 `ERROR_NOT_FOUND`/1168, zero tee writes or confirmed blocked writes); readiness 128 (128 successful cancels, 128 tee-write entries, zero confirmed blocked writes); EOF-to-flush 128 (8 successful first cancels, 120 error 1168, zero writes/confirmed blocked writes); stdin check-to-write one actual in-kernel write cancellation (the check-to-write gap itself had zero attempts). The probe classifies read-to-tee, EOF-to-flush, and stdin-gap as uncovered because those traces do not represent the full production join witness. Watchdog self-test recorded a blocked real pipe, watchdog release, helper reaping, and no child kill. Overall classification remains `rc002=unresolved`: this is neither proof of a defect nor proof of safety, so no production change is justified.

## CMD-AUD-003 Windows 最终实现 SHA 核查

最终实现源码树 `32488efaa2f1e0e8b2d074e97d9667b6fbdadfc7` 已在 `windows-2022` 通过现有 `windows-cancellation-interleaving.yml` workflow 核查：[run 41844950745](https://github.com/qubit-ltd/rs-command/actions/runs/41844950745)，[trace artifact 13331255662](https://github.com/qubit-ltd/rs-command/actions/runs/41844950745/artifacts/13331255662)。执行命令为 `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`：6 项通过、0 项失败、110 项过滤。三平台完整 workflow [41844961106](https://github.com/qubit-ltd/rs-command/actions/runs/41844961106) 也在同一源码 SHA 通过；其 Windows job 的 `test_windows_probe_watchdog_reaps_blocked_child` 测试通过（1 项通过、115 项过滤）。

生产 `CommandIo` 阻塞 read 场景执行一次。取消成功；原 read 返回 Windows 错误 995；reader helper 已 join，进程已回收。trace 记录 `production_join_pending_witness=false`、`original_io_return_is_subsequent_io=false`、`source_writer_manually_released=true`、`watchdog_released=false`，因此未建立生产 cleanup join 挂起／救援的完整链条。有限替代探针记录：read-to-tee 128 次（首次取消成功 45 次、`ERROR_NOT_FOUND`/1168 共 83 次、tee 写入及确认阻塞写均为零）；readiness 128 次（取消成功 128 次、tee 写进入 128 次、确认阻塞写为零）；EOF-to-flush 128 次（首次取消成功 8 次、错误 1168 共 120 次、写入及确认阻塞写为零）；stdin check-to-write 是一次实际内核 write 取消（该间隙本身尝试数为零）。由于这些 trace 未构成完整生产 join 见证，read-to-tee、EOF-to-flush 和 stdin gap 分类为未覆盖。watchdog 自测记录真实 pipe 阻塞、watchdog 释放、helper 回收且未 kill child。总分类仍为 `rc002=unresolved`：这既不证明缺陷，也不证明安全，因此没有依据修改生产代码。

## RSC-003 当前源码核查（2026-11-05）

> 历史源码核查记录。请参阅[最新证据摘要](#current-summary)了解当前源码复核结论。

本节记录本轮有界核查，不覆盖下方历史 run。核查的仓库提交为
`783f96651c20f48fd2529574ffbe37aad09fb7fd`；本轮工作区为 Linux x86_64，
`rustc 1.94.0`、`cargo 1.94.0`。没有在本机运行 Windows 专用测试，也没有把 Linux
筛选结果当作 Windows 验证。

### 当前生产路径与可达性矩阵

| 事件 | 本轮结论 | 证据边界 |
| --- | --- | --- |
| 成功取消 | Windows `IoCancellation::cancel` 会设置取消 token 并调用 `CancelSynchronousIo`；历史真实 Windows trace 观察到它对原始阻塞 read 成功。 | 运行证据来自已记录的 Windows SHA `70c343d155c88852443ae327e37baded29f318d4`，不是本轮 Linux 运行。 |
| 原始操作正常返回 | 未建立。上述真实 trace 中原始 `File::read` 返回 Windows 错误 995，而不是正常完成。 | [CMD-REASSESS-002 trace](#cmd-reassess-002-fresh-windows-evidence-2026-11-04) 记录原始 read 返回和事件顺序。 |
| 后续真实生产 I/O | 源码路径存在取消检查与同步 I/O 入口之间的竞态窗口；本轮未发现新的可达路径假设。历史生产 trace 没有观察到后续 tee write。 | `read_output_inner` 在成功读取并捕获字节后会再次检查 token；普通文件 tee 写入位于该检查之后。源码可达性本身不证明调度实际发生。 |
| 后续 I/O 实际阻塞 | 未建立。既有真实生产 trace 的 tee 写入数为 0；替代 pipe/缓冲 writer 探针不等同于普通文件生产 tee。 | 不将旧探针的部分事件拼接成一次生产见证。 |
| 生产 join pending | 代码在取消成功后直接 join helper；共享 100 ms 确认期限只用于取消请求失败的路径，不限制成功取消后的 join。实际生产 join 因后续 I/O 而 pending 的见证未建立。 | `finish_helpers`、`finish_output_reader` 和 `join_output_reader` 的当前实现证明 join 没有该期限，不证明它在生产场景中挂起。 |
| watchdog 释放 | 未在生产场景观察到。 | 独立 watchdog 自测证明 harness watchdog 可以释放刻意阻塞的真实 pipe；历史生产 trace 记录 `watchdog_released=false`。 |
| helper 与 child 回收 | 历史生产 trace 最终观察到 helper join 和 child reap；独立 watchdog 自测也观察到 helper/child 回收。 | 这些清理事件没有伴随生产 join pending 或 watchdog 释放，不能补成所需因果链。 |

### 结论

本轮源码核对覆盖 `CommandIo::finish_helpers`、`finish_output_reader`、
`OutputReader`、`read_output_inner`、取消 token、Windows 同步 I/O 入口及既有
Windows trace。现有材料已经包含生产阻塞 read trace 和有界的 128 次探针；再运行相同
调度样本不会新增缺失事件。用测试暂停、pipe tee 或 `BufWriter` 人为构造后续阻塞 I/O
会改变待核查的生产路径，因此没有新增实验或行为修复。

缺失的完整见证仍是：成功取消、原操作正常返回、随后进入真实且阻塞的生产 I/O、生产
cleanup join 因此保持 pending，最后由 watchdog 释放并回收 helper/child。本轮分类为
**`unresolved`**：没有证实缺陷，也没有证明生产路径安全。此项有界核查完成；未执行
Windows 验证，因为没有新的生产路径假设需要针对性运行，且当前执行环境为 Linux。

## 归档摘要：中文当前摘要（2026-11-04）

> 历史摘要归档。请参阅[最新证据摘要](#current-summary)；保留本表以追踪当时的评审项映射。

本节归档 2026-11-04 的 CMD-REV-001 至 CMD-REV-004 和 CMD-REASSESS-002 摘要，保留当时记录的评审项映射与证据；当前状态请参阅[最新证据摘要](#current-summary)。

| 评审项 | 当前状态与证据 | 缺口／历史上下文 |
| --- | --- | --- |
| CMD-REV-001 — 示例自包含 | Linux x86_64 在实施源码树 `3d8461a`（基于 checkout `7b2a0adfce45db127ea137ea84268b95d239d5c2`）上验证通过。`CARGO_TARGET_DIR=/tmp/rs-command-target-t1-focused cargo test --test documentation_examples_tests` 通过：harness 4 项测试；生成／选中 12 个示例，其中 6 个 Unix 示例通过，6 个 `compile`／`no_run` 示例完成类型检查但运行时忽略。修复 harness 前，从任一外部 README 删除导入时命令退出 0，因为隐式通配导入掩盖了缺项；修复后两份 README 变异均因 `CommandRunner` 和 `Command` 的 E0433 编译失败。 | Linux 上被 `cfg(windows)` 排除的 Windows 示例既未编译也未运行。详见[本轮本地实施与验证](#current-local-implementation-and-verification-2026-11-04)。 |
| CMD-REV-002 — 整理当前状态与历史证据 | 在当时，该摘要及归档／上下文标注记录了当时可用证据；该文档整理没有产生新的运行时证据。 | 旧编号 CMD-R02、CMD-003、RC-001、RC-002 来自不同评审阶段，未证明彼此一一对应。详见[上下文与归档索引](#archived-evidence-index)。 |
| CMD-REV-003 — 说明 readiness 变体和字段 | 已在 2026-11-04 的任务工作区完成，具体说明见[当时的本地实施与验证记录](#current-local-implementation-and-verification-2026-11-04)。 | 不声称改变平台行为。历史 RC-002 编号仅用于上下文说明。 |
| CMD-REV-004 — 先核查 Windows cancellation/join 疑点 | **有界核查完成；行为仍未解决，不支持修改生产策略。** Windows run [41830652995](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995) 在 `windows-2022` 精确测试任务 SHA `4812685a10616c663ab207351c9812356da36b21`：筛选 6 项、6 项通过、0 失败、过滤 105 项；[trace artifact 13322432766](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995/artifacts/13322432766)。生产场景没有建立所需的因果见证链。 | trace 观察到取消成功和原始 read 错误 995，但 `production_join_pending_witness=false`；原 read 返回并非后续 I/O，source writer 被手动释放，watchdog 未释放。helper 已 join，子进程已回收。详见[本轮本地实施与验证](#current-local-implementation-and-verification-2026-11-04)和[归档 RC-002 结果](#rc-002-windows-cancellation-interleaving-verification)。 |
| CMD-REASSESS-002 — 重新核查 Windows 成功取消与生产 join 疑点 | **新鲜有界 Windows run 已完成；分类仍为 `unresolved`；无依据修改生产代码。** run [41851372381](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381) 在 `windows-2022` 精确测试源码 SHA `70c343d155c88852443ae327e37baded29f318d4`：选中 6 项，6 项通过、0 失败、过滤 110 项；[artifact 13335311428](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381/artifacts/13335311428)。 | 生产尝试没有建立后续阻塞 I/O 或生产 join pending；详见[CMD-REASSESS-002 新鲜 Windows 证据](#cmd-reassess-002-fresh-windows-evidence-2026-11-04)。此行独立于历史 CMD-REV-004。 |

## CMD-REASSESS-002 新鲜 Windows 证据（2026-11-04）

现有 Windows workflow 在 `windows-2022` 对源码 SHA `70c343d155c88852443ae327e37baded29f318d4` 执行了新一轮核查：[run 41851372381](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381)，[artifact 13335311428](https://github.com/qubit-ltd/rs-command/actions/runs/41851372381/artifacts/13335311428)。命令：`cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`；筛选 6 项、6 项通过、0 失败、过滤 110 项。trace SHA-256 为 `66032f5773f2756d954f5e18dcfd17ae2eeae1284ad1af13e9400dbdcc8ff048`。

生产 `CommandIo` 阻塞 read 场景执行一次。观测顺序为：read entry 7.1102 ms、cleanup start 17.3933 ms、成功取消返回 17.7285 ms、原 read 返回 17.7461 ms（原始 OS 错误 995）、cleanup join 17.9244 ms。取消 observer 触发时 cleanup 尚未完成，但 `production_join_pending_witness=false`；`original_io_return_is_subsequent_io=false`。该路径使用普通文件 tee，tee 写入数为 0，也没有进入后续 I/O。最终 helper join 成功、进程已回收；source writer 是手动释放的，watchdog 未释放。故未建立生产 pending join／watchdog 救援链。

有界探针结果：read-to-tee 执行 128 次（取消成功 30 次、错误 1168 共 98 次、tee write 入口 0、确认阻塞写 0、有效部分命中 0）。该探针直接调用 `read_output_inner`，靠二次取消和手动排空 pipe 回收，因此不测试生产 `OutputReader` cleanup join。readiness 执行 128 次（取消成功 128 次、tee write 入口 128、确认阻塞写 0、有效命中 0）；未建立后续阻塞 I/O 或生产 join pending。EOF-to-flush 执行 128 次 pipe-tee 替代尝试（取消成功 45 次、错误 1168 共 83 次、确认阻塞写 0）；生产场景不适用，因为生产 tee 是无缓冲普通 `File`，EOF 后没有待刷出的缓冲数据可形成阻塞 flush。stdin check-to-write 间隙尝试 0 次，因为 Windows `wait_writable` 只在同步 write 前检查取消状态；另一次真实内核 write 取消成功并返回 995，但不能见证该间隙。watchdog 自测确认真实 pipe 阻塞、watchdog 释放、helper 回收、child 回收且未 kill child。

总分类为 **`unresolved`**：没有 trace 满足完整顺序——成功取消、该操作正常返回、后续 I/O 阻塞、生产 cleanup join pending、watchdog 释放及回收。本轮复用了既有观察器和同步点；没有改变生产 writer 或 I/O。无需第二轮实验：当前证据已定位缺失的因果事件；再用缓冲／pipe tee 或人为插入取消后的操作只会构造生产路径不存在的操作，不能增加生产路径证据。既有记录已经包含各适用场景历史 128 次尝试。本记录不把 Linux 上的 `cfg` 运行称作 Windows 验证；本项目未修改生产代码、测试或其他文档。

## Archived evidence index

All earlier evidence sections in this file, including dated source reviews, checkpoint sections, and the older `Result` / final validation sections, are archived records. Their SHAs, workflow runs, artifacts, failures, and observations are preserved as recorded; they are not the current status. Return to the [current summary](#current-summary) for the present conclusion.

| Historical label | Context in this file | Relationship to current scope |
| --- | --- | --- |
| CMD-R02 | Earlier implementation-plan identifier for Windows cancellation/join evidence collection. | Related context for CMD-REV-004; identity is not established solely by the reused label. |
| CMD-003 | Earlier review/plan identifier used in legacy RC-002 verification text. | Its scope depends on the referenced plan stage; it is not assumed to equal current CMD-REV-003. |
| RC-001 | Historical production behavior question and Windows cancellation probe record. | Related behavior evidence for CMD-REV-004, while preserving each checkpoint's scope and limits. |
| RC-002 | Historical documentation verification and cancellation-interleaving report title, reused across stages. | May refer to documentation work or Windows probe history; it is not treated as a single current item. |
| CMD-REV-001–004 summary (2026-11-04) | The preceding English and Chinese status tables preserve an earlier item mapping and evidence snapshot. | Archived context only; the latest evidence summary above is authoritative for current status. |

## Current local implementation and verification (2026-11-04)

The implementation source tree is now recorded by commit `3d8461a`, based on rs-command checkout SHA `7b2a0adfce45db127ea137ea84268b95d239d5c2`; code-affecting checks below ran against the same source content before it was committed, and this status-only document was added afterward. On Linux x86_64, `./.infra/bin/align-ci.sh`, `git diff --check`, `./.infra/bin/style-check.sh`, `cargo check --workspace --all-targets`, and `./.infra/bin/ci-check.sh` all exited 0. The CI script included workspace tests, feature-matrix tests, Rustdoc, Markdown examples, package verification, dependency checks, and coverage. Coverage passed the unchanged thresholds: functions 97.90% (514/525), lines 96.96% (4756/4905), regions 97.11% (6892/7097). Its documentation harness generated 12 examples: 6 passed and 6 compile/no_run cases were type-checked but ignored at runtime. `cargo test --doc` and a separate coverage run were not repeated because `ci-check.sh` ran both Rustdoc and the configured coverage suite successfully.

The full reusable Rust CI workflow [41831220597](https://github.com/qubit-ltd/rs-command/actions/runs/41831220597) completed successfully at task SHA `55931a914a0b4f146023ac955a7580695403beb4`. The `ubuntu-latest`, `windows-2022`, and `macos-15` verify jobs, dependency baseline, default and no-default feature matrices, coverage, and all configured infrastructure suites passed. The workflow marked its optional feature-test, integration-test, compile-only target, fuzz-check, derive-coverage, and Pages jobs as skipped because the caller supplied no corresponding inputs (their reusable-workflow inputs default to empty/false); these skipped jobs are not reported as passing. The repository-local `ci-check.sh` separately exercised the project's configured feature and coverage requirements.

The downstream command `cargo test --manifest-path /data/working/qubit/.worktrees/rs-command/rs-mime-validation/Cargo.toml --lib command_execution` exited 0 (3 passed, 8 filtered). Its first attempt exited 101 because the temporary rs-mime worktree lacked the transitive `../rs-value` sibling path; adding temporary links to the existing sibling repositories fixed the validation layout, and the rerun compiled `qubit-command` through `/data/working/qubit/.worktrees/rs-command/rs-command`, a symlink to the task implementation worktree. No production behavior changed, so the conditional `command_backend` downstream test was not applicable.

T3 documents `Readable(usize)` as the observed next-read byte capacity, bounded further by destination capacity and remaining output budget; Unix reports the 8 KiB poll read cap, Windows reports the `PeekNamedPipe` observation, `Eof` describes the closed-pipe probe, and `Cancelled` does not imply EOF. The enum shape and read behavior did not change. Static branch comparison and the workspace checks above passed.

For CMD-REV-004, Windows run [41830652995](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995) completed on `windows-2022` at exact task SHA `4812685a10616c663ab207351c9812356da36b21`: `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture` selected 6 tests; 6 passed, 0 failed, 105 filtered. The [uploaded trace artifact](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995/artifacts/13322432766) records one production `CommandIo` scenario attempt: native cancellation succeeded, the original read returned error 995, but `original_io_return_is_subsequent_io=false` and `production_join_pending_witness=false`; the source writer was manually released and `watchdog_released=false`. The reader helper joined and the child process was reaped. Thus the full sequence of subsequent real blocked I/O → pending production cleanup join → independent watchdog release → join/recovery was not witnessed; classification remains `unresolved` and does not justify a production strategy change. The 128-attempt read-to-tee probe recorded 17 first-cancel successes and 111 `ERROR_NOT_FOUND` results with zero tee entries, blocked writes, or valid hits, but calls the lower-level reader and is not a production cleanup witness. Readiness recorded 128 cancellation successes and tee entries, with zero blocked writes/valid hits; EOF-to-flush used 128 attempts (7 successful cancels, 121 `ERROR_NOT_FOUND`) and had no tee entries, blocked writes, or valid hits. Both remain substitute-path evidence. The stdin check-to-write gap had zero attempts because it has no pending synchronous operation to cancel; a neighboring in-kernel write cancellation is a different interval. The workflow selected six tests and its nonzero-count guard passed.

## 本轮本地实施与验证（2026-11-04）

本轮实施源码树记录为提交 `3d8461a`，基于 rs-command checkout SHA `7b2a0adfce45db127ea137ea84268b95d239d5c2`；以下代码检查在提交前对相同源码内容执行，状态报告文档随后补入。Linux x86_64 上，`./.infra/bin/align-ci.sh`、`git diff --check`、`./.infra/bin/style-check.sh`、`cargo check --workspace --all-targets` 和 `./.infra/bin/ci-check.sh` 均退出 0。CI 脚本包含 workspace 测试、feature matrix、Rustdoc、Markdown 示例、package 验证、依赖检查和覆盖率。原覆盖率门槛全部通过：函数 97.90%（514/525）、行 96.96%（4756/4905）、区域 97.11%（6892/7097）。其中文档 harness 生成 12 个示例：6 个通过，6 个 compile/no_run 示例完成类型检查但运行时忽略。因 `ci-check.sh` 已成功运行 Rustdoc 与覆盖率套件，没有重复执行 `cargo test --doc` 和独立覆盖率命令。

完整 reusable Rust CI workflow [41831220597](https://github.com/qubit-ltd/rs-command/actions/runs/41831220597) 已在任务 SHA `55931a914a0b4f146023ac955a7580695403beb4` 上成功完成。`ubuntu-latest`、`windows-2022`、`macos-15` verify、dependency baseline、default 和 no-default feature matrix、coverage 以及所有已配置的 infrastructure suites 均通过。由于调用方未提供对应输入（reusable workflow 的输入默认空／false），workflow 将可选 feature-test、integration-test、compile-only target、fuzz-check、derive-coverage 和 Pages jobs 标记为 skipped；这些跳过项不记作通过。仓库本地 `ci-check.sh` 另行执行了项目配置的 feature 与 coverage 验证。

下游命令 `cargo test --manifest-path /data/working/qubit/.worktrees/rs-command/rs-mime-validation/Cargo.toml --lib command_execution` 退出 0（3 项通过，8 项过滤）。首次执行退出 101，原因是临时 rs-mime worktree 缺少传递依赖 sibling path `../rs-value`；补齐指向现有 sibling 仓库的临时链接后重跑成功。编译日志确认 `qubit-command` 通过 `/data/working/qubit/.worktrees/rs-command/rs-command` 解析，该路径是指向本轮实现 worktree 的符号链接。本轮未改变生产行为，因此条件性下游 `command_backend` 测试不适用。

T3 将 `Readable(usize)` 说明为下一次读取采用的字节容量，并受目标 buffer 和剩余总输出预算进一步限制；Unix 报告 8 KiB poll 读取块上限，Windows 报告 `PeekNamedPipe` 观察数量；`Eof` 表示探测到管道关闭，`Cancelled` 不表示 EOF。枚举形状和读取行为未变；静态分支对照及上面的 workspace 检查通过。

CMD-REV-004 已在 Windows run [41830652995](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995) 的 `windows-2022` 上针对任务 SHA `4812685a10616c663ab207351c9812356da36b21` 完成验证：命令 `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture` 筛选 6 项，6 项通过、0 失败、过滤 105 项。[上传的 trace artifact](https://github.com/qubit-ltd/rs-command/actions/runs/41830652995/artifacts/13322432766) 记录生产 `CommandIo` 场景尝试 1 次：原生取消成功，原始 read 返回错误 995，但 `original_io_return_is_subsequent_io=false`、`production_join_pending_witness=false`；source writer 由测试手动释放，`watchdog_released=false`。reader helper 已 join，子进程已回收。因此未见证“后续真实阻塞 I/O → 生产 cleanup join pending → 独立 watchdog 释放 → join/资源恢复”的完整顺序，结论仍为 `unresolved`，不支持修改生产策略。128 次 read-to-tee 探针有 17 次首次取消成功、111 次 `ERROR_NOT_FOUND`，tee 入口、阻塞写及有效命中均为 0；该探针调用较低层 reader，不能代表生产 cleanup 见证。readiness 探针记录 128 次取消成功和 128 次 tee 入口，阻塞写／有效命中均为 0；EOF-to-flush 共尝试 128 次（7 次取消成功、121 次 `ERROR_NOT_FOUND`），tee 入口、阻塞写和有效命中均为 0。两者仍属于替代路径证据。stdin check-to-write 间隙没有待取消同步操作，尝试数为 0；相邻的内核 write 取消属于另一个时间窗口。workflow 的 6 项测试筛选及非零数量保护均通过。

## 归档证据索引

紧随本索引的检查点章节以及文末较早的 `Result`／最终验证章节均为归档记录。原有 SHA、workflow run、artifact、失败及观察均按记录保留，不代表当前状态。请回到[当前摘要](#current-summary)查看本轮结论。CMD-R02、CMD-003、RC-001 和 RC-002 是不同计划／评审阶段的上下文编号；本文不推断没有证据支持的一一对应关系。

## T1 Windows production-path checkpoint (2026-11-04)

> Archived historical evidence. See the [current summary](#current-summary).


The T1 worktree started at source SHA `062305a854471940b56430f7578907d910ffdbf0` on branch `codex/rc-001-rc-002-20261104`. The active host is Linux x86_64 (`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`), with `rustc 1.94.0 (4a4ef493e 2026-03-02)` and `cargo 1.94.0 (85eff7c80 2026-01-15)`. The focused command `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture` exited 0 locally but selected **0 tests** (94 filtered out), because the probe module is Windows-only. This is not Windows runtime evidence; no probe trace or Windows selected-test count was produced for this SHA.

The existing `workflow_dispatch` job targets `windows-2022`. It now records the checkout SHA, platform, command and Cargo's selected-test count in the uploaded artifact, and fails if the filter selects zero tests. The local host has no Windows runtime or `pwsh`; the coordinating agent triggered the committed and pushed probe workflow after confirming GitHub authentication. No source probe or production code was changed.

| Scenario | T1 source inspection / available evidence | Classification at this checkpoint |
| --- | --- | --- |
| Blocked read through production `CommandIo` cleanup | The probe uses `read_output_stream`, a real Windows pipe read, an ordinary-file tee, and `CommandIo::cancel_and_collect`. It observes cancellation and the original read return, then the controller manually releases the source writer. It does not establish a subsequent blocked I/O or watchdog-rescued production join. No fresh Windows trace exists at this SHA. | Implemented observation path; runtime unverified, overall causal chain unresolved. |
| Read-return-to-tee | Existing race probe calls `read_output_inner` directly and uses an anonymous-pipe tee; this is not the production tee/cleanup path. | Substitute-path only; production scenario unresolved. |
| EOF-to-flush | Existing interleaving probe uses a pipe tee. The production tee is an unbuffered `File`; the current probe does not demonstrate a pending production flush. | Production behavior not witnessed; unresolved (pipe observations are not production evidence). |
| Stdin check-to-write | The production Windows path checks cancellation before beginning the synchronous write, so there is no pending synchronous I/O in the gap for `CancelSynchronousIo` to cancel. The probe's in-kernel write cancellation is a different interval. | Gap lacks the stated pending-I/O precondition; the neighboring write interval is not evidence for it. |

The committed Windows run [41816083533](https://github.com/qubit-ltd/rs-command/actions/runs/41816083533) passed on `windows-2022` at source SHA `7bbfc6c04df4dd63929de205eb65d7c446cbe5f5`. The focused command selected 6 tests (6 passed, 0 failed, 105 filtered), and uploaded [trace artifact 13314467975](https://github.com/qubit-ltd/rs-command/actions/runs/41816083533/artifacts/13314467975). The watchdog harness self-test reaped its helper without killing the child.

The production `CommandIo` scenario observed one successful cancellation, the original blocked read returning Windows error 995, and an ordinary-file tee with zero writes. The cleanup worker was unfinished when sampled, but `production_join_pending_witness` was false; the same original read returned before cleanup joined, the source writer was manually released, and the watchdog did not release it. No subsequent blocked I/O or complete production-join causal chain was witnessed, so the scenario remains `unresolved` and does not justify a production fix. The separate 128-attempt read-to-tee probe had 14 successful first cancellations, 114 `ERROR_NOT_FOUND` (1168), zero tee-write entries, zero confirmed blocked writes, and zero valid hits; it directly calls `read_output_inner`, uses a pipe tee, and manually drains it. The readiness probe had 128/128 successful cancels, 128 tee entries, zero confirmed blocked writes and zero valid hits, but uses a pipe tee and does not establish a production join. EOF-to-flush used a pipe tee substitute (128 attempts, 60 successful cancels, 68 `ERROR_NOT_FOUND`, zero tee entries or blocked writes); the production ordinary-file tee has no buffered data to flush. The stdin probe observed one successful cancellation after the synchronous pipe write had entered the kernel (write returned 995); the check-to-write gap itself had zero attempts and no pending synchronous I/O to cancel. These scenarios remain `uncovered`/`unresolved` as described by the trace; zero-hit results do not prove absence of a defect.

The final code commit `44644d668f72c5f00e1266f3d6582acf4ab4e3c8` was also tested by Windows run [41816635646](https://github.com/qubit-ltd/rs-command/actions/runs/41816635646): 6 passed, 0 failed, 105 filtered; `selected_test_count=6`; [trace artifact 13314887584](https://github.com/qubit-ltd/rs-command/actions/runs/41816635646/artifacts/13314887584). Its production `CommandIo` trace again recorded successful cancellation, original read error 995, ordinary-file tee with zero writes, `production_join_pending_witness=false`, manual source-writer release, and no watchdog release. The legacy read-to-tee loop had 40 first-cancel successes and 88 `ERROR_NOT_FOUND` results, with zero tee entries, blocked writes, or valid hits. Readiness had 128 successful cancels and 128 tee entries, but zero blocked writes/hits. The EOF pipe-tee probe had 11 successful cancels, 117 `ERROR_NOT_FOUND` results, and one successful-cancel/flush ordering at attempt 109; that is still a substitute-pipe observation and does not classify the production ordinary-file tee. The stdin probe again observed one successful cancellation after an in-kernel write began, while the check-to-write gap had zero attempts. Overall RC-001 remains unresolved: these runs provide current-source runtime observations but do not establish the suspected production hang or its absence. The full three-platform workflow completed successfully on this source commit (run 41816634209). The workflow count guard prevents its named filter from passing silently with zero tests.

## RC-002 / T4 local implementation verification (2026-11-04)

> Archived historical evidence. See the [current summary](#current-summary).


The marked Markdown harness now requires the selected example IDs and their expected modes in both READMEs and both user guides. On Linux x86_64 with Rust/Cargo 1.94.0, the final `cargo test --manifest-path /data/working/qubit/.worktrees/rs-command/rc-001-rc-002-20261104/Cargo.toml --test documentation_examples_tests` passed (4 integration-crate tests, including two shared-support tests; 12 generated/selected document examples: 6 Unix examples passed and 6 `compile`/`no_run` examples were compiled and ignored at runtime). Windows examples guarded by `cfg(windows)` were excluded on Linux; they were not compiled or run. The final harness supports a separate document root while resolving `qubit-command` from `CARGO_MANIFEST_DIR`, so external document copies exercise the real package.

The mutation command was `DOCUMENTATION_EXAMPLES_ROOT=<external-fixture-root> CARGO_TARGET_DIR=<isolated-target> cargo test --manifest-path <project>/Cargo.toml --test documentation_examples_tests`, run once per fixture. Each final external-copy mutation exited 101 for the intended reason: changing `max_total_output_bytes` in the marked budget example to a nonexistent API produced `no method named`; removing the Chinese README quick-start marker produced `missing required marker quick-start`; adding a second well-formed quick-start marker and Rust fence produced `duplicate marker quick-start`; changing `windows-output` from mode `windows` to `compile` produced `marker windows-output must use mode windows, found compile`. All fixtures and their isolated Cargo target were removed. During diagnosis, an initial API mutation command exited 101 because the harness incorrectly treated a document-only fixture as the package root; this led to separating document-root and package-root paths. An initial duplicate mutation exited 101 at the adjacency guard because two markers were placed together; the final valid duplicate-fence mutation verified duplicate-ID rejection. The first two `align-ci.sh` runs exited 1 on the new Rust headers/import and then test-redirect rule; those issues were fixed before the final alignment passed.

Other final local verification on this worktree:

- `./.infra/bin/align-ci.sh`: exit 0. Two earlier runs identified and led to fixes for the required Rust headers, wildcard import, and test redirect rule; the final alignment and infrastructure check passed.
- `./.infra/bin/ci-check.sh`: exit 0 after the final source/test changes. Style, build, tests, feature matrix, package verification, dependency checks and coverage passed; coverage was 97.90% functions, 97.06% lines, and 97.13% regions, meeting the unchanged thresholds.
- `cargo test --doc`: exit 0, 17 passed. The final `ci-check.sh` also reran doctests after the last harness changes.
- `./.infra/bin/coverage.sh`: exit 0; the unchanged thresholds passed (97.90% functions, 97.06% lines, 97.13% regions).
- `cargo test --manifest-path /data/working/qubit/rust-common/rs-mime/Cargo.toml --lib command_execution`: exit 0, 3 passed, 8 filtered.
- `cargo test --manifest-path /data/working/qubit/rust-common/rs-mime/Cargo.toml --lib command_backend`: exit 0, 4 passed, 7 filtered.

Windows-specific examples were excluded on Linux, so this record provides no evidence that they compiled or ran; a Windows runner and the three-platform workflow remain required. These local results do not replace that platform evidence.

## T1 Windows 生产路径检查点（2026-11-04）

> 历史归档证据。请参阅[当前摘要](#current-summary)。


T1 worktree 起始源码 SHA 为 `062305a854471940b56430f7578907d910ffdbf0`，分支 `codex/rc-001-rc-002-20261104`。执行主机为 Linux x86_64（`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`），工具链为 `rustc 1.94.0 (4a4ef493e 2026-03-02)` 和 `cargo 1.94.0 (85eff7c80 2026-01-15)`。本机执行筛选命令 `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture` 退出 0，但命中 **0 项测试**（过滤 94 项），因为 probe module 仅在 Windows 编译。这不是 Windows runtime 证据；此 SHA 没有新 trace 或 Windows 实际筛选测试数。

现有 `workflow_dispatch` job 运行于 `windows-2022`。现已让上传 artifact 记录 checkout SHA、平台、命令和 Cargo 实际筛选测试数，并在筛选数为零时失败。本机没有 Windows runtime 或 `pwsh`；协调主 Agent 确认 GitHub 已认证后，在代码提交并推送后触发了 Windows workflow。未修改源码 probe 或生产代码。

| 场景 | T1 源码检查／现有证据 | 本检查点分类 |
| --- | --- | --- |
| 生产 `CommandIo` cleanup 中的阻塞 read | probe 使用 `read_output_stream`、真实 Windows pipe read、普通文件 tee 和 `CommandIo::cancel_and_collect`。它观察取消及原始 read 返回，随后由 controller 手动释放 source writer；没有证明后续阻塞 I/O 或 watchdog 救援生产 join。此 SHA 无新 Windows trace。 | 观察路径已实现；runtime 未验证，完整因果链仍未解决。 |
| Read 返回到 tee | 现有竞态 probe 直接调用 `read_output_inner`，并使用匿名管道 tee；这不是生产 tee／清理路径。 | 仅替代路径证据；生产场景未解决。 |
| EOF 到 flush | 现有交错 probe 使用管道 tee。生产 tee 是无缓冲 `File`；当前 probe 未证明生产 flush pending。 | 未见证生产行为；未解决（管道观察不算生产证据）。 |
| Stdin check-to-write | Windows 生产路径在同步 write 开始前检查 cancellation，因此该间隙没有可供 `CancelSynchronousIo` 取消的 pending synchronous I/O。probe 中取消已进入内核的 write 属于不同时间窗口。 | 该间隙缺少所述 pending-I/O 前提；邻接 write 窗口不能作为它的证据。 |

Windows run [41816083533](https://github.com/qubit-ltd/rs-command/actions/runs/41816083533) 在 `windows-2022` 和源码 SHA `7bbfc6c04df4dd63929de205eb65d7c446cbe5f5` 上通过。筛选命令命中 6 项测试（6 通过、0 失败、过滤 105 项），并上传了 [trace artifact 13314467975](https://github.com/qubit-ltd/rs-command/actions/runs/41816083533/artifacts/13314467975)。watchdog harness 自检成功回收 helper，未杀死子进程。

生产 `CommandIo` 场景观察到一次成功取消，原始阻塞 read 返回 Windows error 995，普通文件 tee 写入 0 次。采样时 cleanup worker 尚未完成，但 `production_join_pending_witness` 为 false；同一个原始 read 在 cleanup join 前返回，source writer 由测试手动释放，watchdog 未释放它。没有观察到后续阻塞 I/O 或完整生产 join 因果链，因此该场景仍为 `unresolved`，不支持生产修复。独立 128 次 read-to-tee 探针中，首次取消成功 14 次、`ERROR_NOT_FOUND`（1168）114 次、tee-write 入口 0 次、确认阻塞写 0 次、有效命中 0 次；该探针直接调用 `read_output_inner`，使用 pipe tee 并手动排空。readiness 探针 128/128 次取消成功、128 次 tee 入口、确认阻塞写 0 次、有效命中 0 次，但 tee 是管道，且没有证明生产 join。EOF-to-flush 使用管道 tee 替代品（128 次尝试、60 次取消成功、68 次 `ERROR_NOT_FOUND`、tee 入口和阻塞写均为 0）；生产普通文件 tee 没有缓冲数据可供 flush。stdin 探针观察到一次同步 pipe write 进入内核后被成功取消（write 返回 995）；check-to-write 间隙本身尝试数为 0，也没有可取消的 pending synchronous I/O。trace 将这些场景分类为 `uncovered`／`unresolved`；零命中不能证明缺陷不存在。

最终代码提交 `44644d668f72c5f00e1266f3d6582acf4ab4e3c8` 也在 Windows run [41816635646](https://github.com/qubit-ltd/rs-command/actions/runs/41816635646) 上测试：6 通过、0 失败、过滤 105 项；`selected_test_count=6`；[trace artifact 13314887584](https://github.com/qubit-ltd/rs-command/actions/runs/41816635646/artifacts/13314887584)。生产 `CommandIo` trace 再次记录取消成功、原始 read error 995、普通文件 tee 写入 0 次、`production_join_pending_witness=false`、source writer 手动释放且 watchdog 未释放。旧 read-to-tee 循环中首次取消成功 40 次、`ERROR_NOT_FOUND` 88 次，tee 入口、阻塞写和有效命中均为 0。readiness 中 128 次取消成功、128 次 tee 入口，但阻塞写／有效命中均为 0。EOF 管道 tee 探针中取消成功 11 次、`ERROR_NOT_FOUND` 117 次，并在第 109 次观察到一次成功取消与 flush 顺序；这仍是替代管道观察，不能分类生产普通文件 tee。stdin 探针再次观察到同步 write 进入内核后的成功取消，而 check-to-write 间隙尝试数为 0。RC-001 整体仍未解决：这些运行提供当前源码的 runtime 观察，但既未建立疑似生产挂起，也未证明其不存在。完整三平台 workflow 已在该源码提交上成功完成（run 41816634209）。workflow 测试数保护会阻止命名 filter 零测试时静默通过。

## RC-002 / T4 本地实施验证（2026-11-04）

> 历史归档证据。请参阅[当前摘要](#current-summary)。


实际 Markdown harness 现在要求双语 README 和双语用户指南中列出的示例 ID 及其预期模式均匹配。在 Linux x86_64、Rust/Cargo 1.94.0 上，最终执行 `cargo test --manifest-path /data/working/qubit/.worktrees/rs-command/rc-001-rc-002-20261104/Cargo.toml --test documentation_examples_tests` 通过（集成测试 crate 4 项通过，其中包括 2 项共享支持模块测试；生成 12 个适用于本平台的文档示例，6 个 Unix 示例通过，另外 6 个 `compile`／`no_run` 示例完成类型检查但运行时忽略。`cfg(windows)` 示例在 Linux 上被平台条件排除，没有编译或运行）。最终 harness 将外部文档目录与 `CARGO_MANIFEST_DIR` 中实际的 `qubit-command` crate 分开，因此外部文档副本会真实编译项目依赖。

变异命令为 `DOCUMENTATION_EXAMPLES_ROOT=<外部副本目录> CARGO_TARGET_DIR=<隔离 target> cargo test --manifest-path <项目>/Cargo.toml --test documentation_examples_tests`，每个副本各运行一次。最终四项外部副本变异均以预期原因退出 101：将预算示例中的 `max_total_output_bytes` 改为不存在的 API，报 `no method named`；删除中文 README 快速开始标记，报 `missing required marker quick-start`；增加第二个格式正确的快速开始标记及 Rust fence，报 `duplicate marker quick-start`；将 `windows-output` 的模式从 `windows` 改成 `compile`，报 `marker windows-output must use mode windows, found compile`。变异副本及独立 Cargo target 均已删除。诊断中，最初仅复制文档的 API 变异命令因 harness 把文档目录当成 package 根目录而退出 101，由此修正为分离文档路径和 package 路径。最初的重复标记变异命令也退出 101，但错误来自相邻性检查；最终使用有效的第二个 fence 验证了重复 ID 拒绝。前两次 `align-ci.sh` 均退出 1，分别发现新 Rust 文件头部／导入问题及测试重定向规则问题；修复后最终对齐通过。

本 worktree 最终本地验证：

- `./.infra/bin/align-ci.sh`：退出 0。此前两次运行分别发现并修正许可证头、通配导入及测试重定向规则问题；最终对齐与基础设施检查通过。
- `./.infra/bin/ci-check.sh`：最终源码／测试修改后退出 0。Style、构建、测试、feature matrix、package 验证、依赖检查和覆盖率均通过；覆盖率为函数 97.90%、行 97.06%、区域 97.13%，原门槛未变。
- `cargo test --doc`：退出 0，17 项通过。最终 `ci-check.sh` 也在最后一次 harness 修改后重跑了 doctest。
- `./.infra/bin/coverage.sh`：退出 0；原覆盖率门槛通过（函数 97.90%、行 97.06%、区域 97.13%）。
- `cargo test --manifest-path /data/working/qubit/rust-common/rs-mime/Cargo.toml --lib command_execution`：退出 0，3 项通过、8 项过滤。
- `cargo test --manifest-path /data/working/qubit/rust-common/rs-mime/Cargo.toml --lib command_backend`：退出 0，4 项通过、7 项过滤。

Windows 示例未在 Linux 主机执行；仍需 Windows runner 和三平台 workflow。以上本地结果不能替代平台证据。

## T3 implementation-checkout audit (2026-11-04)

> Archived historical evidence. See the [current summary](#current-summary).


This audit was performed in the implementation worktree on branch `codex/outputs-files-windows-20261104`, HEAD `65ccacbf79a49a845af16c02fc0bd6cf59e3b752` (clean at audit start). The active host is Linux x86_64 (`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`), with `rustc 1.94.0 (4a4ef493e 2026-03-02)` and `cargo 1.94.0 (85eff7c80 2026-01-15)`. No Windows runtime is available in this environment. The focused Windows command, `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`, was therefore **not run** here; there is no fresh selected-test count for this implementation checkout. The previously recorded Windows Server 2022 run at SHA `e76e1700894717523c24c74fe95b94b3809eeaa7` is historical evidence for that SHA, not a fresh run at this checkout SHA.

The current workflow `.github/workflows/windows-cancellation-interleaving.yml` is a `workflow_dispatch` job on `windows-2022`, installs Rust 1.94.0, prepares local path dependencies, and invokes that focused command while retaining the log as an artifact. The production-path probe in `src/command_runner/internal/output_collector/tests/windows_probe_tests.rs` uses `read_output_stream`, an ordinary-file tee, and `CommandIo::cancel_and_collect`; it records a successful native cancellation and original read return when observed, but explicitly marks `original_io_return_is_subsequent_io:false` and `production_join_pending_witness:false`. Its controller releases the source writer manually. It therefore does not itself establish a post-cancel blocked I/O, watchdog rescue of a pending production join, or the complete failure chain. The separate legacy probe in `src/command_runner/internal/windows_cancel_interleaving_tests.rs` uses the older direct collector path and likewise cannot upgrade substitute-path observations to production-path evidence.

For CMD-R03 in the current implementation plan, the evidence-based classification remains **incomplete / unresolved**. Blocked-read cancellation is distinguishable from read-return-to-tee; the latter still lacks a witnessed subsequent real blocked I/O and pending production cleanup join. EOF-to-flush still uses a pipe tee substitute; the production tee is an unbuffered `File`, so the recorded probe does not classify a blocking flush there. The stdin check-to-write gap has no pending synchronous I/O for `CancelSynchronousIo` to cancel; an observed cancellation of a later in-kernel write would belong to a different interval. No full causal chain is available for a production fix, and no production behavior is changed by this audit. Live Windows T3 execution and its test count remain outstanding.

## T3 实施工作区审查（2026-11-04）

> 历史归档证据。请参阅[当前摘要](#current-summary)。


本次审查在实施 worktree 中进行，分支为 `codex/outputs-files-windows-20261104`，HEAD 为 `65ccacbf79a49a845af16c02fc0bd6cf59e3b752`（审查开始时工作区干净）。当前主机为 Linux x86_64（`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`），工具链为 `rustc 1.94.0 (4a4ef493e 2026-03-02)` 和 `cargo 1.94.0 (85eff7c80 2026-01-15)`。当前环境没有 Windows runtime。因此本次**未运行** Windows 筛选命令 `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`；此实施 checkout 没有新的已选测试数。此前记录的 SHA `e76e1700894717523c24c74fe95b94b3809eeaa7` 上 Windows Server 2022 运行属于该 SHA 的历史证据，不是本 checkout 的新运行。

当前 workflow `.github/workflows/windows-cancellation-interleaving.yml` 使用 `workflow_dispatch`，运行于 `windows-2022`，安装 Rust 1.94.0、准备本地路径依赖，并执行上述筛选命令且将日志保存为 artifact。生产路径探针 `src/command_runner/internal/output_collector/tests/windows_probe_tests.rs` 使用 `read_output_stream`、普通文件 tee 和 `CommandIo::cancel_and_collect`；在观察到时记录原生成功取消和原 read 返回，但明确标记 `original_io_return_is_subsequent_io:false` 与 `production_join_pending_witness:false`。controller 手动释放 source writer。因此该探针本身没有证明取消后另一个真实阻塞 I/O、watchdog 救援 pending production join 或完整故障因果链。独立旧探针 `src/command_runner/internal/windows_cancel_interleaving_tests.rs` 仍走较早的 direct collector 路径，不能将替代路径观察升级为生产路径证据。

当前实施计划中的 CMD-R03 基于现有证据仍为**未完成／未解决**。阻塞 read 的取消与 read 返回到 tee 是不同窗口；后者仍缺少“后续真实阻塞 I/O”和“生产 cleanup join pending”的见证。EOF-to-flush 仍使用管道 tee 替代品；生产 tee 是无缓冲 `File`，现有探针没有分类该路径的阻塞 flush。stdin check-to-write 间隙没有可供 `CancelSynchronousIo` 取消的待处理同步 I/O；之后进入内核的 write 被取消属于另一个区间。没有完整因果链支持生产修复，本次审查未修改生产行为。实时 Windows T3 执行和测试数仍待补齐。

## Final Windows probe (2026-11-04)

> Archived historical evidence. See the [current summary](#current-summary).


The task checkout `e76e1700894717523c24c74fe95b94b3809eeaa7` passed the Windows Server 2022 focused workflow, [run 41799652557](https://github.com/qubit-ltd/rs-command/actions/runs/41799652557), and uploaded [artifact 13306199684](https://github.com/qubit-ltd/rs-command/actions/runs/41799652557/artifacts/13306199684). The production-path test passed (1 passed, 0 failed); the focused workflow's six selected tests passed. The production trace recorded a successful cancellation of the blocked read (`cancel_raw_os_error: 995`), read return, zero tee writes, and completed cleanup. Although the cleanup thread was sampled as unfinished at the cancellation observer, the trace correctly records `production_join_pending_witness:false`; the controller manually released the source writer and `watchdog_released:false`. No subsequent blocked I/O or watchdog-rescued production join was witnessed. The read-to-tee, EOF-to-flush, and stdin check-to-write cases remain uncovered under their recorded preconditions; overall CMD-R02 remains **unresolved**, so this evidence does not justify a production fix.

The first Windows workflow attempt (run 41798634838) and the first full CI matrix (run 41798642860) failed to compile the probe because it referenced undeclared `serde_json`. The probe now formats the trace with the standard library and adds no dependency. The corrected Windows workflow above passed. The corrected full CI matrix passed on `e76e1700894717523c24c74fe95b94b3809eeaa7`: [Rust CI run 41799659323](https://github.com/qubit-ltd/rs-command/actions/runs/41799659323). Its Windows 2022, Ubuntu latest, and macOS 15 verification jobs, feature matrix, coverage, and configured infrastructure suites all passed.

## 最终 Windows 探针（2026-11-04）

> 历史归档证据。请参阅[当前摘要](#current-summary)。


任务 checkout `e76e1700894717523c24c74fe95b94b3809eeaa7` 已通过 Windows Server 2022 专项 workflow：[run 41799652557](https://github.com/qubit-ltd/rs-command/actions/runs/41799652557)，并上传了 [artifact 13306199684](https://github.com/qubit-ltd/rs-command/actions/runs/41799652557/artifacts/13306199684)。生产路径测试通过（1 项通过、0 项失败）；专项 workflow 筛选的 6 项测试全部通过。生产 trace 记录了对阻塞 read 的成功取消（`cancel_raw_os_error: 995`）、read 返回、tee 写入 0 次以及 cleanup 完成。尽管取消 observer 采样时 cleanup 线程尚未完成，trace 正确标记 `production_join_pending_witness:false`；controller 手动释放 source writer，且 `watchdog_released:false`。未观察到后续阻塞 I/O，也未观察到 watchdog 救援生产 join。根据已记录前提，read-to-tee、EOF-to-flush 和 stdin check-to-write 仍未覆盖；CMD-R02 整体仍为**未解决**，因此这份证据不支持修改生产代码。

首次 Windows workflow（run 41798634838）和首次完整 CI 矩阵（run 41798642860）因探针引用未声明的 `serde_json` 而编译失败。探针现改用标准库格式化 trace，没有新增依赖。修正后的 Windows workflow 已通过。修正后的完整 CI 矩阵已在 `e76e1700894717523c24c74fe95b94b3809eeaa7` 上通过：[Rust CI run 41799659323](https://github.com/qubit-ltd/rs-command/actions/runs/41799659323)。Windows 2022、Ubuntu latest、macOS 15 验证任务、feature matrix、coverage 和已配置的 infrastructure suites 均通过。

## T2 production-path review (2026-11-04, worktree checkpoint)

> Archived historical evidence. See the [current summary](#current-summary).


The implementation checkpoint is based on worktree HEAD `65ec714f0224f07f80dd732a018021404b518105` on `codex/cmd-win-cancel-20261104`; the current source and verification-document changes are uncommitted, so this base SHA is not a SHA for the new probe. The production cleanup route is `CommandIo::cancel_and_join` / `cancel_and_collect` → `finish_helpers` (batch cancellation, then sequential bounded finishes) → `finish_output_reader` → `OutputReader::join` → `join_output_reader`. A new bounded Windows harness case uses `read_output_stream` to create the real reader helper, configures an ordinary temporary `File` tee, and invokes `CommandIo::cancel_and_collect` with a real Windows `ExitStatus`; a test-only observer records the native cancellation result, read return, tee write entries, and cleanup completion. It performs one coordinated blocked-read observation per run (below the 128-attempt cap), not a randomized race sweep. The older probes still call `read_output_inner` directly and their anonymous-pipe tee observations remain substitute-path evidence.

At this checkpoint the required focused command was run on Linux x86_64 with Rust 1.94.0: `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture` exited 0 but selected **0 tests** (92 filtered out). This is not Windows evidence. `pwsh` is absent. The Windows GNU cross-check was also attempted, but stopped in the `alloca` build script because `x86_64-w64-mingw32-gcc` is unavailable; it did not compile or execute the Windows test. A Windows `workflow_dispatch` was subsequently run after pushing the task ref; its exact SHA, run ID, artifact and test count are recorded in the Final Windows probe section above.

The new production-path probe distinguishes cancellation of the original blocked read from a subsequent blocked I/O: canceling the blocked read and observing its return is not a subsequent-I/O witness. Its ordinary-file tee is configured and instrumented, but any zero tee-write count only establishes this particular observed schedule. The cleanup-thread state sampled when the cancellation observer fires is diagnostic scheduler timing only; the trace explicitly sets `production_join_pending_witness=false`. The probe controller manually releases the source writer; this is not watchdog rescue. The existing outer process watchdog only recovers a hung probe process. The read-return-to-tee window still requires a successful native cancellation plus a following real blocked operation to establish the reported defect; a test wrapper pause or an anonymous-pipe tee is not a substitute. Windows `wait_writable` only checks the cancellation token before the stdin write, so the check-to-write gap has no pending synchronous I/O for `CancelSynchronousIo` to cancel; a write that blocks after the gap is not itself evidence of successful cancellation in that gap. The EOF/flush probe still uses a pipe substitute and does not classify the ordinary-file tee. The final Windows run recorded results but did not establish the full sequence (successful cancel, original I/O return, subsequent real blocked I/O, pending production `CommandIo` cleanup join, watchdog release, completed join, and helper/process reaping); CMD-R02 therefore remains `unresolved`.

| Production scenario | Review result at `65ec714f0224f07f80dd732a018021404b518105` | Classification |
| --- | --- | --- |
| Blocked output read through production cleanup | New bounded harness case uses `read_output_stream`, an ordinary-file tee, and `CommandIo::cancel_and_collect` with an actual `ExitStatus`; it records native cancel result and original read return. The final Windows run observed cancellation of the original blocked read, its return, zero tee writes, and completed cleanup. This original read is not a subsequent-I/O witness. | Observed schedule completed; unresolved. |
| Read-return-to-tee | The ordinary-file tee is now part of a production-path probe, but the required successful-cancel/read-return/subsequent-blocked-I/O sequence was not observed. Existing pipe-tee probes are substitutes. | Uncovered; unresolved. |
| Stdin check-to-write | Production Windows `wait_writable` checks the token before write; no synchronous I/O is pending in that gap. Existing probe cancels an entered kernel write, which is a different window. | Gap lacks a pending-I/O precondition; unresolved. |
| EOF-to-flush | Existing probe uses a pipe tee; no ordinary-file tee trace records a blocked flush after EOF. | Production window not witnessed; unresolved. |

## T2 生产路径审查（2026-11-04，工作树检查点）

> 历史归档证据。请参阅[当前摘要](#current-summary)。


实施检查点基于工作树 HEAD `65ec714f0224f07f80dd732a018021404b518105`、分支 `codex/cmd-win-cancel-20261104`；当前源码和验证文档修改尚未提交，因此该基线 SHA 不是新探针的 SHA。生产 cleanup 调用链为 `CommandIo::cancel_and_join`／`cancel_and_collect` → `finish_helpers`（批量取消，再按顺序执行有界收尾）→ `finish_output_reader` → `OutputReader::join` → `join_output_reader`。新增有界 Windows harness 场景经 `read_output_stream` 创建真实 reader helper，配置普通临时 `File` tee，并以真实 Windows `ExitStatus` 调用 `CommandIo::cancel_and_collect`；test-only observer 记录原生取消结果、read 返回、tee write 进入次数和 cleanup 完成。每次运行仅进行一次同步的阻塞 read 观察（低于 128 次上限），不是随机竞态扫描。旧探针仍直接调用 `read_output_inner`，其匿名管道 tee 观察仍属于替代路径证据。

所需筛选命令已在 Linux x86_64、Rust 1.94.0 运行：`cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture` 退出码为 0，但命中 **0 项测试**（过滤 92 项），不是 Windows 证据。环境中没有 `pwsh`。Windows GNU 交叉检查也已尝试，但在 `alloca` build script 阶段因缺少 `x86_64-w64-mingw32-gcc` 停止，没有编译或执行 Windows 测试。仓库存在 `.github/workflows/windows-cancellation-interleaving.yml` 的 `workflow_dispatch` workflow。之后已推送 task ref 并运行 Windows workflow；精确 SHA、run ID、artifact 和测试数量见上方“最终 Windows 探针”章节。

新增生产路径探针区分“取消原先阻塞的 read”和“后续另一个阻塞 I/O”：观察被取消的原 read 返回不等于后续 I/O 见证。探针配置并计数普通文件 tee，但零 write 只代表本次具体调度观察。取消 observer 通知时采样的 cleanup 线程状态只是调度诊断；trace 明确记录 `production_join_pending_witness=false`。probe controller 手动释放 source writer，不代表 watchdog 救援；现有外层 process watchdog 仅在 probe 进程挂起时负责回收。read 返回到 tee 的窗口仍需真实 Windows 证据同时观察成功取消、原 read 返回和后续真实阻塞操作；测试 wrapper 暂停或匿名管道 tee 不能替代。Windows `wait_writable` 在 stdin write 前检查取消 token，因此 check-to-write 间隙没有待取消的同步 I/O；间隙后开始阻塞的 write 也不能证明该间隙发生成功取消。EOF／flush 探针仍使用管道替代路径，不能分类普通文件 tee。真实 Windows run 已记录各场景，但没有证明完整因果顺序（成功取消、原 I/O 返回、后续真实阻塞 I/O、生产 `CommandIo` cleanup join pending、watchdog 释放、join 完成以及 helper／进程回收），因此 CMD-R02 仍为 `unresolved`。

| 生产场景 | `65ec714f0224f07f80dd732a018021404b518105` 的审查结果 | 分类 |
| --- | --- | --- |
| 生产 cleanup 中的阻塞 output read | 新增有界 harness 经 `read_output_stream`、普通文件 tee 和 `CommandIo::cancel_and_collect`（传入真实 `ExitStatus`）；记录原生取消结果和原 read 返回。每次运行一次协调观察，不超过 128 次上限；最终 Windows run 观察到取消原 read、其返回、tee 写入为零且 cleanup 完成。被取消的原 read 不等于后续 I/O 见证。 | 已观察该调度；未解决。 |
| Read 返回到 tee | 普通文件 tee 已加入生产路径探针，但尚无成功取消／原 read 返回／后续阻塞 I/O 的观察。现有管道 tee 探针为替代路径。 | 未覆盖；未解决。 |
| Stdin check-to-write | Windows 生产 `wait_writable` 在 write 前检查 token；该间隙没有待处理同步 I/O。现有探针取消的是已进入 kernel 的 write，属于另一窗口。 | 该间隙缺少待取消 I/O 前提；未解决。 |
| EOF 到 flush | 现有探针使用管道 tee；没有普通文件路径记录 EOF 后阻塞 flush。 | 未见证生产窗口；未解决。 |

## English

CMD-003 in the current implementation plan (the legacy report calls this RC-002) remains **unresolved**. The Windows Server 2022 workflow passed on source SHA `dd2f2280cd69f62e7c860718163ff9cf3afeae66`: [workflow run 41788417690](https://github.com/qubit-ltd/rs-command/actions/runs/41788417690), [uploaded probe log](https://github.com/qubit-ltd/rs-command/actions/runs/41788417690/artifacts/13302277144). It ran `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture` (5 passed, 0 failed). The log records the exact checkout SHA and scenario counts below.

The run exercised the Windows probe module after CMD-001's source relocation and preserved the full self-start test name. The watchdog self-test blocked and released a real pipe helper, reaped it, and did not kill the child. This validates harness recovery only. The read-to-tee probe still uses `read_output_inner` with an anonymous-pipe tee and manual recovery, not the ordinary-file tee plus production `OutputReader` cleanup join. Readiness uses production readiness and a real source `File::read`, but does not establish a subsequent blocked I/O or pending production join. The stdin probe successfully cancelled an in-kernel write; it did not witness a cancellation in the check-to-write gap. The EOF probe's tee is a pipe substitute; its flush observations do not represent production's unbuffered `File` tee. No scenario satisfies the complete witness: successful cancellation → original I/O completion → subsequent blocked I/O → pending production cleanup join → watchdog release and helper/process reaping. No production cancellation change is justified; the finding remains unresolved, and zero-hit or substitute-path evidence is not proof of safety.

| Scenario | Windows evidence from run 41788417690 | Coverage and status |
| --- | --- | --- |
| Read-to-tee | 128 attempts; 46 successful cancellations + 82 `ERROR_NOT_FOUND` (1168); 0 tee-write entries, 0 partial-probe hits, 0 confirmed blocked writes; 1 source-writer broken pipe. Probe calls `read_output_inner` directly and recovers with a second cancellation plus manual pipe drain. | Does not cover production `OutputReader` cleanup join; unresolved. |
| Output readiness to read | 128 attempts; 128 successful cancellations + 0 `ERROR_NOT_FOUND`; 128 tee-write entries; 0 confirmed blocked writes and 0 valid hits. It uses production readiness and a real source `File::read`, but the tee/cleanup path does not provide the required production join witness. | No later blocked I/O plus pending production join established; unresolved. |
| Stdin check-to-write | 1 observed attempt; cancellation succeeded during the real pipe write, which returned `ERROR_OPERATION_ABORTED` (995); 0 post-cancel write entries. The actual check-to-write gap had 0 attempts in the scenario classification. | The helper's check/wait-to-write gap remains uncovered; unresolved. |
| EOF-to-flush | 128 pipe-tee substitute attempts; 74 successful cancellations + 54 `ERROR_NOT_FOUND`; no confirmed blocked writes. Three successful cancel/flush orderings were recorded at attempts 21, 41, and 76, but the applicable production-path classification is 0 attempts because production uses an unbuffered `File` tee. | Substitute-path observations are not production evidence; unresolved. |

The overall classification is `rc002=unresolved`. This run does not establish that a production cleanup join blocks after successful cancellation, and it does not prove the absence of such a defect. Historical runs, SHAs, failures, and prior evidence below remain preserved; missing historical metrics remain **not recorded**, not zero.

## 中文

当前实施计划中的 CMD-003（本报告历史编号为 RC-002）仍为**未解决**。Windows Server 2022 workflow 在源码 SHA `dd2f2280cd69f62e7c860718163ff9cf3afeae66` 上通过：[workflow run 41788417690](https://github.com/qubit-ltd/rs-command/actions/runs/41788417690)，[上传的探针日志](https://github.com/qubit-ltd/rs-command/actions/runs/41788417690/artifacts/13302277144)。执行命令为 `cargo test --lib windows_successful_cancel_interleaving -- --test-threads=1 --nocapture`（5 项通过，0 项失败）。日志记录了精确 checkout SHA 和下表场景计数。

本次运行在 CMD-001 源码归位后执行 Windows 探针模块，完整自启动测试名仍有效。watchdog 自检阻塞并释放了真实管道 helper，成功回收 helper，且未杀死子进程；这只验证 harness 的恢复能力。read-to-tee 探针仍直接调用 `read_output_inner`，tee 使用匿名管道并通过手动方式恢复，不是普通文件 tee 加生产 `OutputReader` cleanup join。readiness 场景使用生产 readiness 和真实 source `File::read`，但没有证明后续 I/O 阻塞及生产 join pending。stdin 场景成功取消了已进入内核的 write，没有见证 check-to-write 间隙中的取消。EOF 场景使用管道 tee 替代品；其 flush 观察不代表生产无缓冲 `File` tee。没有任何场景满足完整见证链：取消成功 → 原 I/O 完成 → 后续 I/O 阻塞 → 生产 cleanup join pending → watchdog 释放并回收 helper／子进程。现有证据不支持修改生产取消策略；疑点保持未解决，零命中及替代路径证据均不构成安全证明。

| 场景 | Run 41788417690 的 Windows 证据 | 覆盖范围与状态 |
| --- | --- | --- |
| read-to-tee | 128 次尝试；46 次成功取消 + 82 次 `ERROR_NOT_FOUND`（1168）；0 次 tee-write 进入、0 次部分命中、0 次确认阻塞写入；1 次 source writer broken pipe。探针直接调用 `read_output_inner`，通过第二次取消和手动排空管道恢复。 | 未覆盖生产 `OutputReader` cleanup join；未解决。 |
| output readiness 到 read | 128 次尝试；128 次成功取消 + 0 次 `ERROR_NOT_FOUND`；128 次 tee-write 进入；0 次确认阻塞写入、0 个有效命中。使用生产 readiness 和真实 source `File::read`，但 tee／清理路径未提供所需的生产 join 见证。 | 未证明后续 I/O 阻塞且生产 join pending；未解决。 |
| stdin check-to-write | 观察到 1 次尝试；真实管道 write 期间成功取消，write 返回 `ERROR_OPERATION_ABORTED`（995）；取消后 0 次 write 进入。场景分类中实际 check-to-write 间隙尝试数为 0。 | helper 的 check/wait 到 write 间隙仍未覆盖；未解决。 |
| EOF 到 flush | 管道 tee 替代路径尝试 128 次；74 次成功取消 + 54 次 `ERROR_NOT_FOUND`；无确认阻塞写入。记录到第 21、41、76 次尝试中成功取消与 flush 的顺序命中，但生产路径适用分类为 0 次，因为生产使用无缓冲 `File` tee。 | 替代路径观察不构成生产证据；未解决。 |

总分类为 `rc002=unresolved`。本次运行未证明成功取消后生产 cleanup join 会阻塞，也不能证明不存在该缺陷。下文历史 runs、SHA、失败和既有证据均保留；历史记录缺失的指标仍为**未记录**，不能记作零。

# REV-004 follow-up evidence (2026-11-04)

> Archived historical evidence. See the [current summary](#current-summary).


## Current implementation checkpoint

> Archived historical evidence. See the [current summary](#current-summary).


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

> 历史归档证据。请参阅[当前摘要](#current-summary)。


### 当前实现检查点

> 历史归档证据。请参阅[当前摘要](#current-summary)。


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

> Archived historical evidence. See the [current summary](#current-summary).


## Result

> Archived historical evidence. See the [current summary](#current-summary).


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

> Archived historical evidence. See the [current summary](#current-summary).


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

> Archived historical evidence. See the [current summary](#current-summary).


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

> Archived historical evidence. See the [current summary](#current-summary).


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

> Archived historical evidence. See the [current summary](#current-summary).


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

> 历史归档证据。请参阅[当前摘要](#current-summary)。


## 结果

> 历史归档证据。请参阅[当前摘要](#current-summary)。


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

> 历史归档证据。请参阅[当前摘要](#current-summary)。


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

> 历史归档证据。请参阅[当前摘要](#current-summary)。


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

> 历史归档证据。请参阅[当前摘要](#current-summary)。


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

> 历史归档证据。请参阅[当前摘要](#current-summary)。


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

## Final RC-001 / RC-002 validation (2026-11-04)

> Archived historical evidence. See the [current summary](#current-summary).


Final implementation source commit `44644d668f72c5f00e1266f3d6582acf4ab4e3c8` passed the full
Rust CI workflow [run 41816634209](https://github.com/qubit-ltd/rs-command/actions/runs/41816634209):
`ubuntu-latest`, `windows-2022`, and `macos-15` verification jobs succeeded, as did the configured
feature matrix, coverage, dependency baseline, and infrastructure checks. The dedicated Windows
cancellation workflow [run 41816635646](https://github.com/qubit-ltd/rs-command/actions/runs/41816635646)
also passed and selected all 6 focused tests.

The Markdown examples are now extracted from the maintained bilingual README and user-guide files
and compiled in an isolated harness against the actual package. Locally,
`cargo test --test documentation_examples_tests` passed (the harness ran 12 cases: 6 Linux examples
passed and 6 `compile`/`no_run` examples were type-checked but ignored at runtime; Windows-only examples were excluded by `cfg(windows)`). External-document mutation checks
confirmed failures for an invalid API call, a missing marker, a duplicate marker, and a platform mode
mismatch. The full local `.infra/bin/ci-check.sh` passed after the final change; coverage was 97.90%
functions, 97.02% lines, and 97.11% regions.

On the Windows runner, the production `CommandIo` cancellation path was exercised. The blocked
`File::read` returned Windows error 995 after cancellation, and no later tee write was observed.
However, the worker was not shown to remain blocked on the production cleanup join until watchdog
release. Other targeted scenarios either used pipe substitutes or had no cancellable operation in the
check-to-write gap. The observations therefore do not establish either the suspected production hang
or its absence. RC-001's behavior question remains unresolved; no production behavior change was
justified by this evidence. RC-002's documentation compilation and platform validation goal is
complete.
