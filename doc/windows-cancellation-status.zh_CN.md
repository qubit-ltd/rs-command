# Windows 取消状态

**状态：unresolved（未解决）。** runner 在取消成功后可能无期限地 join helper。已检查的生产路径和现有探针没有证明：取消成功后发生的阻塞生产 I/O 会使该 join 持续等待。本轮既未证实，也未排除生产挂起，更不证明 Windows 安全。

`qubit-command` 0.12 的受管 API 增加了进程树管理和独立 settlement 报告，但没有改变 helper join 的证据，也没有建立 Windows 上的硬返回期限。`Managed` 进程树策略不代表每个 helper join 都有界；受管命令同样受此限制。应用应检查 settlement 和清理报告；确实需要硬期限时，应通过独立进程边界监督 runner。

## 集成后源码核查（源码 SHA `a8fab3aed3a0999b6049dcf344d2e075d63e0ea5`，2026-11-24）

本次有界核查运行于 Linux x86_64（`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`）、Rust `1.94.0` 和 Cargo `1.94.0`。针对生产 helper 取消/join、Windows 探针、workflow 和 PowerShell 身份门禁路径执行定向 `git diff --exit-code c871a25024adb72c1f4bdac7f9e226fb03463cb8 --`，退出码为 0。`command -v pwsh` 和 `command -v powershell` 均退出码为 1。本 SHA 未运行 Windows runtime 测试、PowerShell 身份门禁或新 trace。

集成后的入口路径会观察受管 operation start gate，并把它的取消句柄交给 `RunningCommand`；受管文件 preparation 和 commit 路径也有变化。定向比较中，原生 helper 取消实现和 `cancel_and_join` 路径没有变化：Windows 取消成功后仍直接 join；共享 100 ms 确认期限仍只用于取消失败的情形。Windows 探针、workflow 和身份门禁均未相对已记录的 `c871a250` 原生运行改变。该较早 SHA 的运行只属于其自身源码版本，没有覆盖集成后的入口与文件 handoff 路径。

**结论：** RC-006 仍为 **unresolved（未解决）**。集成后源码 SHA 没有同次运行 trace 建立计划要求的生产因果链；源码比较既未证实也未排除生产挂起，也不证明 Windows 安全。未修改生产取消代码或探针。现有 `windows-2022` workflow 与 PowerShell 身份门禁仍是 runtime 证据入口；最终远端证据必须绑定到准确推送的源码 SHA。

## 契约边界

在 Windows 上，`CancelSynchronousIo` 成功表示取消请求成功，不表示 helper 已停止。runner 在发出取消请求前设定一个共享的 100 ms helper 确认截止时间；只有取消请求失败时才检查该时间，并且不会为每个 helper 重置。请求成功后直接 join，不受该期限约束。100 ms 不是整个 `run()` 调用的硬期限。

调用方应保留主错误及其 `cleanup_failures()`，并在把保留输出视为完整内容前检查 `stdout_complete()` 和 `stderr_complete()`。必须保证硬返回期限的应用应通过独立进程边界监督 runner；runner 的 timeout 不提供该保证。

## 源码有界核查（源码 SHA `36579c289a9cd6e9fb981854e87a7e3137cb6512`，2026-11-23）

本次核查运行于 Linux x86_64、Rust `1.94.0`。针对生产取消/helper、Windows 探针、workflow 和身份门禁路径执行 `git diff --exit-code fcdccb7 --`，退出码为 0：这些路径相对最近一次记录的源码核查未变化。`command -v pwsh` 与 `command -v powershell` 均退出码为 1。本 SHA 未运行 Windows runtime 测试、PowerShell 身份门禁，也未生成新的 Windows trace。

现有生产路径探针仍未见证后续阻塞的生产 I/O，也未见证因此等待的生产 cleanup join；记录字段仍包括 `original_io_return_is_subsequent_io=false`、`production_join_pending_witness=false`、`source_writer_manually_released=true` 和 `watchdog_released=false`。基于管道的交错探针及独立 watchdog 自测不能证明普通文件 tee 或生产 stdin 的因果链。现有 `windows-2022` workflow 和 PowerShell 身份门禁仍是 runtime 运行及判读入口，但本环境无法运行它们。

**结论：** RC-006 仍为 **unresolved（未解决）**。本次源码比较只确认受检路径自 `fcdccb7` 起没有变化；既未证实也未排除疑似生产挂起，也不证明 Windows 安全。未修改生产代码或探针。

## 原生 Windows workflow 证据（源码 SHA `c871a25024adb72c1f4bdac7f9e226fb03463cb8`，2026-11-24）

[Windows Cancellation Interleaving run](https://github.com/qubit-ltd/rs-command/actions/runs/45305695014) 在 `windows-2022`（Windows Server 2022）成功完成。共选中 14 项测试：12 项 `windows_` 测试和 2 项 `command_io` 测试，全部通过。PowerShell identity-gate fixtures 通过；结果门禁报告 12 项必需测试均已发现且通过。[Probe artifact](https://github.com/qubit-ltd/rs-command/actions/runs/45305695014/artifacts/14880568265) 包含运行、测试列表、command-I/O 和门禁日志。

同一次运行中的生产路径记录 `production_command_io_blocked_read` 观察到原生取消成功（`cancel_success=true`），随后原始阻塞 read 以原始 OS 错误 `995` 返回，接着 cleanup join、helper join 和 child 回收。记录同时显示 `original_io_return_is_subsequent_io=false`、`production_join_pending_witness=false`、`tee_write_entries=0`、`source_writer_manually_released=true` 和 `watchdog_released=false`；分类仍为 `unresolved`。因此 trace 未观察到取消后的生产 tee 写入/flush 或 stdin 写入进入并持续阻塞，也未观察到生产 cleanup join 因这类后续 I/O 而等待独立 watchdog 释放。

独立 EOF/flush 与 readiness 探针均没有 valid hit。readiness 管道探针记录 128 次成功取消，但没有确认阻塞写入。stdin check-to-write 探针观察到取消时已经在途的 write 以原始 OS 错误 `995` 返回，且没有 post-cancel write entry。这些探针结果不能建立生产文件或生产 stdin 的因果链。单独的 watchdog 自测回收了自己的 child，但没有见证或释放生产 cleanup join。

**结论：** RC-006 仍为 **unresolved（未解决）**。本次原生运行的筛选测试和身份门禁均通过，但没有取得同一 helper 上计划要求的完整顺序：成功取消、原 I/O 返回、后续生产 I/O 持续阻塞、cleanup join 等待该 I/O、独立 watchdog 释放及资源回收。先前 `36579c2` 的 Linux 源码核查保留为历史证据。

## 源码有界核查（源码 SHA `dec767dff01f9fb6c97c923628d472f9a81f33cd`，2026-11-24）

本次核查运行于 Linux x86_64（`Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026`）、Rust `1.94.0` 和 Cargo `1.94.0`。针对生产取消/helper、Windows 探针、workflow 和身份门禁路径执行 `git diff --exit-code c871a25024adb72c1f4bdac7f9e226fb03463cb8 --`，退出码为 0。`command -v pwsh` 和 `command -v powershell` 均退出码为 1。本 SHA 未运行 Windows runtime 测试、PowerShell 身份门禁，也未生成新的 Windows trace。

检查的生产路径中，Windows 取消成功后仍会直接 join helper，不受确认期限约束。生产探针记录 `production_join_pending_witness=false`、`original_io_return_is_subsequent_io=false`、`source_writer_manually_released=true` 和 `watchdog_released=false`；管道探针不能证明生产文件或生产 stdin 的因果链。现有 `windows-2022` workflow 仍是可用的 runtime 入口：固定 Rust `1.94.0`，运行 Windows 与 command-I/O 测试筛选，并应用 PowerShell 身份门禁。本次核查未触发该 workflow。

**结论：** RC-006 仍为 **unresolved（未解决）**。本次源码比较只确认受检路径与记录的 `c871a250` run 一致；既未证实也未排除疑似生产挂起，也不证明 Windows 安全。未修改生产代码或探针，也未生成 artifact。

## 实施阶段有界源码复核（源码 SHA `5fa0cae06001e07b5053171216b1b9f7da6acb15`，2026-11-18）

本次实施阶段源码检查运行于 Linux x86_64、Rust `1.94.0`。`src/execution/executor.rs` 中的 retry timer 测试改动未改变 Windows 取消、helper I/O 或 join 路径；停止诊断改动仅涉及文档。针对生产取消/helper、Windows 探针、workflow 和身份门禁路径执行 `git diff --exit-code fcdccb7 --`，退出码为 0。系统未安装 `pwsh` 或 `powershell`，因此未运行 Windows runtime 测试、PowerShell 门禁或新的 Windows trace。

本次没有发现新的生产可达取消/交错假设，也未触发条件性修复。RC-006 仍为 **unresolved（未解决）**：源码比较既未证实也未排除疑似挂起，也不证明 Windows 安全。此前证据及其边界见[核查历史](rc-002-verification.md#current-summary)。

## 当前实现源码有界核查（源码 SHA `fcdccb7`，2026-11-18）

本次核查在 Linux x86_64、Rust `1.94.0` 环境进行。针对生产取消/收尾、输出、stdin、Windows 探针、workflow 和身份门禁路径执行
`git diff --exit-code 23085b070bd42c7e77c14f22390a7df7eafae767 -- <上述路径>`，退出码为 0。源码阅读及 `rg -n` 检查覆盖 `command_io.rs`、`io_cancellation.rs`、`stdin_pipe.rs`、`output_collector.rs`，以及本轮修改的 session channel 停止与背压路径。`command -v pwsh` 和 `command -v powershell` 均退出码为 1。本 SHA 未运行 Windows runtime 测试、PowerShell 身份门禁，也未生成新的 Windows trace。

生产取消控制流仍如以下历史证据所述：Windows `CancelSynchronousIo` 成功时，output-reader 或 stdin-writer join 仍可能尚未返回。output reader 在读取后、tee I/O 前检查取消状态；生产 tee 是普通文件。stdin 在每轮 readiness/write 前检查取消状态。本轮将 managed-session 字节通道改为容量背压以保留已排队字节，并在停止输出时唤醒阻塞 writer。用户态 channel 通知与 Windows 原生同步 I/O 取消相互独立，不改变 helper join 的证据。

**结论：** 未发现新的生产可达交错假设。RC-006 仍为 **unresolved（未解决）**；本次源码核查既未证实也未排除生产挂起，也不证明 Windows 安全。未修改 Windows 专用代码或探针。

## 前一版实现源码有界核查（源码 SHA `e8c7e8a0fc2a4da33b85ab98d172f9aca1112fa8`，2026-11-17）

本次核查在 Linux x86_64、Rust `1.94.0` 环境进行。针对生产取消/收尾、输出、stdin、Windows 探针、workflow 和身份门禁路径，与评审基线 `fa506a6fb75bc6bcfd3537c5be54b4cda911815e` 执行源码比较，退出码为 0。系统未安装 `pwsh` 或 `powershell`（两条 `command -v` 均退出码为 1）。本 SHA 未运行 Windows runtime 测试、PowerShell 身份门禁，也未生成新的 Windows trace。

| 事件 | 源码观察 | 证据边界 |
| --- | --- | --- |
| 取消与 join | Windows `IoCancellation::cancel` 先发布取消标记，再调用 `CancelSynchronousIo`。收尾先请求取消 stdout、stderr 和 stdin helper，再分别完成它们。取消请求成功时直接进入对应 join；仅取消请求失败后才检查共享的 100 ms 确认期限。 | 成功取消后的 join 没有期限，但仅凭控制流不能证明它会持续挂起。 |
| 输出读取与 tee | reader 保留一次成功读取返回的字节、检查取消状态，随后仍可能同步写 tee。生产 tee 是普通 `File`；EOF 后可能执行 `flush`。 | 源码存在检查与 I/O 之间的调度窗口，但没有同一次运行的证据证明成功取消后接着发生阻塞生产 I/O 并导致生产 join 挂起。 |
| Stdin | helper 在每轮 readiness/write 前检查取消状态；Windows readiness 检查的是取消 token。 | 未建立生产可达的“取消成功后接着阻塞写入”时序。 |
| 探针与 watchdog | 生产探针记录 `original_io_return_is_subsequent_io=false`、`production_join_pending_witness=false`、`source_writer_manually_released=true` 和 `watchdog_released=false`。阻塞管道 watchdog 自测属于另一条 trace。 | 现有证据没有在同一条 trace 中连接取消成功、后续阻塞生产 I/O、等待中的 cleanup join、独立 watchdog 释放以及 helper/进程回收。 |

**结论：** 没有发现新的生产可达交错假设，因此不触发条件性生产修复。RC-006 仍为 **unresolved（未解决）**：疑点既未证实也未排除，本次源码核查不证明 Windows 安全。未修改生产代码或探针。

## 源码 SHA `76fda78d3c4e8961a21193f749bf1228b50929a4` 的证据

本次有界源码审阅运行于 Linux x86_64，Rust 版本为 `1.94.0`。相对所记录 SHA 对生产代码、探针、workflow 和身份门禁文件执行源码比较，退出码为 0，未发现差异。系统没有 `pwsh` 或 `powershell`；已安装的 Windows GNU 编译目标不能提供 Windows 运行环境。本 SHA 未运行 Windows 测试、PowerShell 门禁，也没有生成新的 Windows trace。

| 路径 | 已确认的控制流 | 证据边界 |
| --- | --- | --- |
| 取消与清理 | `IoCancellation::cancel` 先发布 token，再调用 Windows `CancelSynchronousIo`。`finish_helpers_with_wait` 先对 stdout、stderr、stdin 请求取消，再收尾 helper。取消成功后直接 join output reader 或 stdin writer；共享期限用于取消失败分支。 | 确认成功取消后的 join 无期限，不证明该 join 会阻塞。 |
| stdout/stderr 与 tee | `read_output_inner` 在读取后、tee 写入前检查取消状态。生产 tee 是普通 `File`；交错探针使用人为填满的匿名 pipe。 | 探针的 pipe 不能证明生产文件路径会以相同方式阻塞。没有同一条 trace 将成功取消、后续阻塞生产 I/O 和等待中的生产 join 联系起来。 |
| stdin | `write_stdin_until_cancelled` 在每次 readiness/write 前检查取消状态。Windows 探针的满 pipe 不是生产 `ChildStdin` 路径。 | 未建立成功取消后再发生阻塞写入的生产可达时序。 |
| 探针与 watchdog | 现有探针没有在同一条 trace 中组合成功取消、后续阻塞生产操作、由该操作导致的 cleanup join 等待、独立 watchdog 释放以及 helper/进程回收。 | 测试身份核对和独立 watchdog 自测不能证明这条因果链。 |

**结论：** 未发现新的生产可达交错假设，因此未触发 T3 的条件性回归与修复。RC-002 仍为 unresolved；未修改生产代码或探针。结论仅适用于本轮检查的路径和时序。

## 验证记录

- `./.infra/bin/align-ci.sh`：退出码 0；格式与对齐成功。
- `git diff --exit-code 76fda78d3c4e8961a21193f749bf1228b50929a4 -- src/command_runner/internal/command_io.rs src/command_runner/internal/io_cancellation.rs src/command_runner/internal/io_cancellation_token.rs src/command_runner/internal/cancel.rs src/command_runner/internal/cancellable_reader.rs src/command_runner/internal/output_collector.rs src/command_runner/internal/output_reader.rs src/command_runner/internal/stdin_pipe.rs src/command_runner/internal/stdin_writer.rs src/command_runner/internal/pollable_stdin.rs src/command_runner/internal/windows_cancel_interleaving_tests.rs src/command_runner/internal/output_collector/tests/windows_probe_tests.rs .github/workflows/windows-cancellation-interleaving.yml .github/scripts/assert-windows-probe-results.ps1 .github/scripts/assert-windows-probe-results.tests.ps1`：退出码 0；相对核查基线无变化。
- `./.infra/bin/style-check.sh`：退出码 0。
- `cargo check --workspace --all-targets`：退出码 0。
- `./.infra/bin/ci-check.sh`：退出码 0；项目测试、doctest、文档示例、feature matrix、发布包验证和覆盖率均通过。覆盖率：函数 96.95%、行 96.82%、区域 96.74%。
- `cargo package --list --allow-dirty`：退出码 0；通过既有 `/doc/**` 打包规则包含两份双语状态页。
- `uname -a`：退出码 0；Linux x86_64。
- `rustc --version`：退出码 0；`rustc 1.94.0 (4a4ef493e 2026-03-02)`。
- `command -v pwsh` 与 `command -v powershell`：均不可用（退出码 1）；未运行 Windows 专用测试和 PowerShell 身份门禁。
- `cargo test --lib windows_ -- --show-output --test-threads=1 --color never` 与 `cargo test --lib command_io::tests::test_error_not_found_ -- --show-output --test-threads=1 --color never`：未运行；当前主机为 Linux，本地没有 Windows runner。

## 历史记录与更新条件

早期审计和 Windows 运行记录见 [RC-002 验证历史](rc-002-verification.md#current-summary)，每条仅适用于其记载的源码 SHA 与环境。今后只有同一次运行、同一个 helper 的有序 trace 同时关联实际生产路径、原生取消成功、被取消操作返回、后续阻塞生产 I/O 入口及未返回见证、因此等待的生产 cleanup join、独立 watchdog 释放以及 helper/进程回收，才可更新状态。探针注入不得制造生产路径不存在的操作。实验失败或未复现本身不能解决该疑点。

English summary: [Windows cancellation status](windows-cancellation-status.md).
