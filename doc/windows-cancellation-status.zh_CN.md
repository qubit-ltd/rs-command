# Windows 取消状态

**状态：unresolved（未解决）。** runner 在取消成功后可能无期限地 join helper。已检查的生产路径和现有探针没有证明：取消成功后发生的阻塞生产 I/O 会使该 join 持续等待。本轮既未证实，也未排除生产挂起，更不证明 Windows 安全。

## 契约边界

在 Windows 上，`CancelSynchronousIo` 成功表示取消请求成功，不表示 helper 已停止。runner 在发出取消请求前设定一个共享的 100 ms helper 确认截止时间；只有取消请求失败时才检查该时间，并且不会为每个 helper 重置。请求成功后直接 join，不受该期限约束。100 ms 不是整个 `run()` 调用的硬期限。

调用方应保留主错误及其 `cleanup_failures()`，并在把保留输出视为完整内容前检查 `stdout_complete()` 和 `stderr_complete()`。必须保证硬返回期限的应用应通过独立进程边界监督 runner；runner 的 timeout 不提供该保证。

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
