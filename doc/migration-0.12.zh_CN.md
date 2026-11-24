# 迁移到 qubit-command 0.12

[English user guide](user_guide.md) · [中文用户手册](user_guide.zh_CN.md) · [English migration guide](migration-0.12.md) · [English README](../README.md) · [中文 README](../README.zh_CN.md)

本文说明如何从 0.11 迁移到 0.12 的受管执行入口。本文对应 `qubit-command` 0.12 版本；Cargo 依赖约束为 `qubit-command = "0.12"`。原来的 `CommandRunner::run`、`run_with` 和基于路径的 stdin/tee 配置继续保留；只需在应用需要统一并发准入、进程树生命周期或资源结算时，迁移对应调用点。

## 选择入口

简单的一次性命令可以继续使用旧同步入口：

<!-- doc-example:migration-entry-selection:compile -->
```rust
use std::time::Duration;
use qubit_command::{Command, CommandRunner};

let timeout = Duration::from_secs(5);
let command = Command::new("program");
let output = CommandRunner::new(timeout).run(command)?;
```

服务中需要由 `OperationManager` 管理命令时，创建一个共享 executor：

<!-- doc-example:migration-executor-setup:compile -->
```rust
use qubit_command::{CommandExecutionConfig, CommandExecutor};
use qubit_operation::{ManagerOptions, OperationManager};

let manager = OperationManager::new(ManagerOptions::default())?;
let executor = CommandExecutor::new(manager, CommandExecutionConfig::default())?;
```

简单收集结果使用 `CommandRunner::run_managed` 或 `run_managed_in`。需要在返回后继续追踪操作时，使用 `CommandExecutor::submit`、`submit_in` 或带资源的对应方法，并保留返回的 `CommandOperation`。

## 受管路径与工作目录

managed stdin、stdout tee 和 stderr tee 的相对路径以提交时的父进程目录为基准；child 工作目录仍由 command override 优先于 runner 默认值决定。曾依赖 child cwd 定位 tee 的调用点，应传入显式绝对路径。Windows drive-relative 路径（例如 `C:logs\run.txt`）保留平台语义，不等同于 `C:\logs\run.txt`，也不作为普通相对路径拼到父目录。compatibility snapshot 的 `environment` 字段保留但为空；进程仍按启动时继承环境，并应用显式清空、删除和覆盖。

## 进程树策略

新的 managed API 中，`CommandRunOptions::default()` 的 `ProcessTreePolicy` 是 `Managed`。它会把后代进程纳入停止和回收边界，不依赖是否配置 timeout 或 cancellation。只有确实只希望负责直接子进程时才显式选择：

<!-- doc-example:migration-tree-policy:compile -->
```rust
use qubit_command::{CommandRunOptions, ProcessTreePolicy};

let options = CommandRunOptions::new()
    .with_process_tree_policy(ProcessTreePolicy::DirectChild);
```

这项默认值描述的是 0.12 managed 执行；不要把它理解为旧 `run`/`run_with` 生命周期的改变。各平台进程管理能力仍受操作系统约束，特别是 Windows helper 取消边界，见[Windows 取消状态](windows-cancellation-status.zh_CN.md)。

## timeout 与取消

现有 `CommandRunner::new(timeout)` 和 `CommandCancellation` 仍可用于对应执行入口。timeout 仍从子进程启动后开始计时；它不覆盖准备、操作系统 spawn 或所有 OS I/O，也不承诺整个 API 调用有硬墙钟返回期限。timeout 与 cancellation 是停止原因；进程及其 helper 的实际回收仍应通过 outcome 之后的 settlement 判断。

`CommandExecutor` 的 `submit` 是立即尝试提交，拒绝时返回 `CommandSubmitFailure`，其中保留 runner、command、options 和拒绝原因。资源型提交对应 `CommandResourceSubmitFailure`。若要等待领域容量，可选 `submit_wait_blocking`；启用 `async` 后还可使用 `submit_wait`。等待容量不会延长最初的执行期限。

0.12 的提交失败类型会把可恢复的 `runner`、`command`、资源型 `options` 字段装在 `Box` 中，以限制错误值在 `Result` 中的内存尺寸。字段仍保存完整输入；借用时方法调用会自动解引用，若要取回所有权则使用 `*failure.runner`、`*failure.command` 或 `*failure.options`。`FilePreparationFailure.prepared` 同样是 `Box<PreparedFiles>`，取回时可用 `*failure.prepared`。这是新增 0.12 API 的字段类型调整，不影响 0.11 入口。

## 有所有权的文件选项

需要把已经打开的文件交给一次 managed run 时，使用 `CommandResourceRunOptions`：

<!-- doc-example:migration-owned-file-configuration:compile -->
```rust
use std::fs::File;
use qubit_command::{CommandResourceRunOptions, FileSinkPolicy};

let input_file = File::open("input.txt")?;
let output_file = File::options().create(true).write(true).open("output.txt")?;
let resources = CommandResourceRunOptions::new()
    .stdin_file(input_file) // 从当前游标读取，不隐式 seek
    .stdout_sink(output_file, FileSinkPolicy::Truncate);
```

每个输出 sink 都必须指定 `Preserve` 或 `Truncate`。`Truncate` 会在容量与资源预留成功后产生截断副作用；拒绝发生在预留前时，不应假定文件已被改写。冲突校验按已打开文件句柄的身份执行；路径字符串只是查找/诊断信息，并不代表库持有该路径的独占租约。调用方的其他文件句柄副本也不会因所有权移交而自动关闭。

资源输入/输出选项不可 `Clone`，因为它们按值移交文件句柄。迁移时不要把旧路径型 `stdin_file`、`tee_stdout_to_file` 或 `tee_stderr_to_file` 误当成新的 owned-file 接口；旧入口仍按旧语义工作。

## 准入等待与准备错误

提交前会先检查互斥配置；最终 commit 阶段再检查实际打开句柄的文件身份，并在截断或启动前拒绝身份冲突。owned stdin 不能与命令文件或字节 stdin 并用，需选择一种输入来源。session stdin 使用管道，不能与文件 stdin 并用，但仍可使用受支持的字节前缀。tee 路径可能在后续 commit 错误前已创建；若较早的 sink 已截断，后续失败不会回滚它。文件准备错误可通过 `CommandErrorKind::FilePreparationFailed` 及 `CommandErrorReason::FilePreparationFailed` 查看实际阶段、方向和 source。

容量等待期间的 scope stop 对应 `AdmissionError::Stopped`，deadline 到期对应 `CapacityFailure::DeadlineExpired`。已注册 waiter 遇到 executor 关闭时以 `Capacity(Closed)` 结束。`max_waiters` 同时统计本地容量等待和 manager `Busy` 等待；在同一个等待判断中，deadline 优先于 scope stop，scope stop 优先于关闭。新增的 `CommandSubmitFailureReason::Wait(CommandSubmitWaitFailure)` 区分共享等待者额度耗尽 `WaiterLimit` 与计时器失败 `Timer(RetryTimerFailure)`；timer failure 包括启动失败、队列满和断连。穷举匹配 `CommandSubmitFailureReason` 的调用方需补充 `Wait` 分支。

## 阶段报告与错误处理

managed operation 有两个需要分别观察的阶段：

1. `wait_outcome()` 取出唯一的业务 outcome：成功、命令错误、停止或 operation 执行失败。
2. `wait_settlement()` 返回资源结算报告：资源已释放、已转交，或需要恢复。

命令成功不等于 helper、端点或 owned 文件已经释放。若 settlement 为 `RecoveryRequired`，先检查 manager 总报告的 status、effects、计数及溢出/截断信息，再检查存在时的 `external_settlement`，从 adapter 报告中查看其状态、effects、计数和实际 recovery handles。adapter 可能已经执行恢复逻辑，因此外部报告的 `recovery_count` 大于零时，manager 的 `recoveries` 仍可能为空；只对实际存在的 handle 重试或转交。根据 effect 与释放证据决定何时清理暂存文件，不要只因 manager `recoveries` 为空就删除。流式 session 还应在 settlement 前关闭 stdin、并发 drain stdout/stderr，取 outcome 后释放端点。

直接使用 handle 时，operation outcome 中的 `Failed(CommandError)` 保留领域错误；`Stopped` 不带 `CommandOutput`，不能自行补造退出码。operation observer 错误和 settlement 错误表示观察过程未完成，应作为生命周期问题处理。便捷入口 `run_managed` 将成功 outcome 映射成 `CommandOutput`，命令失败映射为 `CommandError`；提交拒绝或 operation 桥接失败也会被转换为 `ManagedOperationFailed` 诊断。若调用方需要区分“提交被拒绝”和“命令本身失败”，使用 `submit` 并检查 `CommandSubmitFailureReason`，不要使用收集型便捷入口。

进程启动前已接受的停止通过 operation handle 表现为 `OperationOutcome::Stopped`，不是普通命令错误；等待 settlement 后，对具体 handle 调用一次 `take_stop_diagnostic()`。收集型 `run_managed*` 入口继续采用原有 `CommandError` 映射。`None` 仍可能出现在已经停止的操作中；诊断若含捕获字节，应检查截断、EOF 完整性和 `cleanup_failures`，并避免默认记录原始敏感内容。完整的双语操作步骤和可编译示例见[英文指南](user_guide.md#observe-outcome-and-settlement-separately)与[中文指南](user_guide.zh_CN.md#分开观察命令结果和资源结算)。

| 旧调用习惯 | 0.12 建议 | 注意事项 |
| --- | --- | --- |
| `CommandRunner::run` / `run_with` | 原样保留，或按服务边界迁移到 `run_managed` | 旧同步调用契约不因新增 API 自动改变。 |
| 用 `CommandCancellation` 停止运行 | 继续使用，managed 调用再观察 settlement | stop 请求和资源释放是两个不同阶段。 |
| 通过路径传 stdin/tee 文件 | 旧路径接口照旧；需要文件所有权时改用 `CommandResourceRunOptions` | 新接口保留当前游标并要求显式 sink 策略。 |
| 假定退出后立即删除临时文件 | 等待 `wait_settlement()` 报告释放后再删除 | `RecoveryRequired` 时执行恢复而非清理。 |

可执行的 collect、session、shutdown 和文件迁移示例见[英文指南](user_guide.md#managed-execution-for-service-work)和[中文指南](user_guide.zh_CN.md#面向服务任务的受管执行)。
