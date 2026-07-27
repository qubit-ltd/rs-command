# Qubit Command

[![Rust CI](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-command/coverage-badge.json)](https://qubit-ltd.github.io/rs-command/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-command.svg?color=blue)](https://crates.io/crates/qubit-command)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

面向 Rust 的命令行进程运行工具库。

## 概览

Qubit Command 提供一个小而明确的结构化 API，用于运行外部程序、捕获 stdout/stderr、控制超时，并用清晰的错误类型报告命令执行失败。

## 功能

- 使用 program + args 的结构化命令执行方式。
- 在确实需要 shell 解析时，提供显式 shell 命令支持。
- 支持配置超时、工作目录、stdin、环境变量和成功退出码。
- 为已管理终端信号或关闭请求的应用提供显式、一次性的取消句柄。
- 设置超时或取消句柄时，基于 Unix process group 和 Windows Job Object 尝试终止进程树。
- 默认保留 stdout 和 stderr 的原始字节，同时提供严格和有损 UTF-8 文本访问方法。
- 支持按流限制内存捕获字节数，并把完整输出流式写入文件。
- 可选择在命令成功但内存输出被截断时返回错误。
- 在截断任何 tee 文件前检查 stdin、stdout 和 stderr 的文件冲突。
- 日志和诊断里的命令文本会对敏感 argv、显式环境变量覆盖、shell
  脚本体以及调用方追加的敏感字段做脱敏展示。
- 使用明确错误类型表示进程启动失败、超时、输出读取失败和非预期退出码。

## 超时行为

`CommandRunner::new()` 默认应用 `DEFAULT_COMMAND_TIMEOUT`（当前为十秒）。需要
不同的命令时长限制时，请调用 `timeout(Duration)`；只有确实需要无限等待时才调用
`without_timeout()`。
超时从子进程成功启动后开始计时，命令准备与启动过程耗费的时间不计入该上限。准备阶段会
打开配置的 stdin 和 tee 路径；打开 FIFO、设备或其他特殊文件时，可能要等待外部对端或
设备就绪，并且该等待不受命令 timeout 限制。

每次轮询会先检查直接子进程，再检查 deadline；观察到子进程退出后，输出收集仍受同一
timeout 限制。达到超时会启动进程树终止和清理，但不保证 `run()` 在该墙钟时长内返回；
平台终止操作和 I/O 辅助线程清理可能需要额外时间。如果后代进程脱离了受管的 Unix
process group 或 Windows Job Object，同时仍持有继承的 I/O 管道，runner 可能要等到
该管道关闭后才能返回。

设置超时或取消句柄后，runner 会尝试终止整个进程树：Unix 平台把命令放入新的
process group，Windows 平台把命令放入 Job Object。

超时测量和休眠使用可注入的 `qubit-clock` timer，因此单元测试可以用手动单调时钟
驱动超时逻辑。未设置超时且未配置取消句柄时，runner 会直接等待进程结束，不进行轮询。
设置超时或取消句柄时，命令执行会同步等待 timer，因此 timer 后端必须能在调用线程阻塞时
独立推进。Tokio timer 不应依赖仅由同一调用线程驱动的 current-thread runtime。

## 取消

`CommandCancellation` 是供已经拥有关闭或终端信号策略的应用使用的一次性句柄。将其 clone
后配置给 runner，再从该策略中调用 `cancel()`。如果在一次运行开始前已观察到取消请求，
runner 不会准备或启动命令，而是返回 `CommandError::CancelledBeforeStart`；否则 runner
会终止受管进程树，并以 `CommandError::Cancelled` 返回保留输出。本 crate 刻意不安装
全局信号处理器。

即使调用 `without_timeout()`，配置取消句柄也会启用进程树管理。支持取消的等待会轮询所配置的
timer；请选择能独立于调用线程持续推进的 timer 后端。

## 大输出

默认情况下，stdout 和 stderr 每个流最多保留
`DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM`（当前为 1 MiB）；如果成功命令的保留输出发生
截断，runner 会返回 `CommandError::OutputTruncated`。如果命令可能输出大量日志，可以
降低内存上限并把完整输出 tee 到文件：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .max_output_bytes(64 * 1024)
    .fail_on_output_truncation(false)
    .tee_stdout_to_file("stdout.log")
    .tee_stderr_to_file("stderr.log")
    .run(Command::new("cargo").arg("test"))?;

if output.stdout_truncated() {
    eprintln!("stdout was truncated in memory; see stdout.log for the full stream");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`bounded_output(max_bytes)` 会为两个流设置新的上限，并保留默认的截断拒绝策略。如果允许
只保留部分内存输出，可组合 `max_output_bytes(max_bytes)` 与
`fail_on_output_truncation(false)`。只有确认输出量有限的可信命令才应显式取消限制：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .unbounded_output()
    .run(Command::new("cargo").arg("test"))?;

assert!(!output.stdout_truncated());
# Ok::<(), Box<dyn std::error::Error>>(())
```

即使保留输出发生截断，非预期退出、超时或取消仍是优先错误。这四类错误都可以通过
`CommandError::output()` 取得保留输出。

## 快速开始

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::new("git").args(&["status", "--short"]))?;

println!("{}", output.stdout_text()?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Shell 命令

优先使用结构化命令：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::new("printf").arg("hello"))?;

assert_eq!(output.stdout_text()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

只有在明确需要 shell 解析、重定向、变量展开或管道时，才使用
`Command::shell`：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::shell("printf hello | tr a-z A-Z"))?;

assert_eq!(output.stdout_text()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

结构化参数会避免 shell 解析，但目标程序仍会按照自己的规则解析选项。当路径或其他值可能
以 `-` 开头时，应使用该程序支持的选项终止符（通常是 `--`），或遵循其文档规定的参数
传递方式。

## 诊断脱敏

Runner 日志、`CommandError::command()` 和 `Command` 的 `Debug` 输出都会通过
`qubit-redact` 生成遮盖后的命令文本。类似 `--password secret`、
`--access-token=...`、`OPENAI_API_KEY=...` 的结构化 argv 值会被遮蔽；显式设置的
环境变量覆盖也只展示遮盖后的 `KEY=value`。`Command::shell` 的脚本体不做 shell
语法解析，统一作为不透明 secret 遮盖。

当默认策略不够时，可以向 runner 注入完整的不可变策略：

下面的示例需要直接声明 `qubit-redact = "0.3"` 依赖，因为 `qubit-command` 不会
重导出属于 `qubit-redact` 的类型。

```rust
use qubit_command::{Command, CommandRunner};
use qubit_redact::{RedactionPolicy, Sensitivity};

let policy = RedactionPolicy::builder()
    .raise("tenant_option", Sensitivity::Secret)
    .allow_exact("username")
    .build()?;
let error = CommandRunner::new()
    .diagnostic_redaction_policy(policy)
    .run(Command::new("__missing__").arg("--tenant-option").arg("secret"))
    .expect_err("sample command should fail");

assert_eq!(
    error.command(),
    r#"["__missing__", "--tenant-option", "<redacted>"]"#,
);
```

对于客户文件路径等位置参数，请使用 `Command::sensitive_arg` 或
`Command::sensitive_arg_os`。原值仍会不加修改地传给子进程，但诊断中只显示配置的
秘密掩码。

Runner 策略只影响 runner 日志和 `CommandError::command()`。
独立的 `Command` `Debug` 输出没有 runner 上下文；每次格式化时都会取得进程级全局
默认策略的快照，只有尚未安装全局默认策略时才使用标准策略。对于确认过的精确字段名
误报，可使用 `allow_exact`；只有在明确接受更宽泛的后缀放行时才使用
`allow_suffix`。放行会让匹配的 argv 或环境变量值原样出现在诊断中，因此每条规则都应
经过安全审阅。

命令生命周期日志使用 `debug` 级别。调用 `disable_logging(true)` 会抑制这些日志；
无法通过 `CommandError` 返回的清理失败仍可能使用 `error` 级别记录。

`CommandOutput` 的 `Debug` 输出会遮盖两个捕获流，只报告字节数、截断标志、退出状态和
耗时。捕获到的 stdout/stderr 字节、显式访问方法以及 tee 文件仍然是进程原始输出。
如果命令输出本身可能包含敏感信息，请配置捕获上限，并在调用方按业务语义过滤。

工作目录、stdin 文件和 tee 文件路径不会出现在 `Debug`、`Display` 或
`CommandError` 的诊断文本中；需要按错误类型处理时，结构化错误字段仍保留原始路径。

## 输出文本

`stdout()` 和 `stderr()` 返回保留下来的原始字节。需要严格 UTF-8 文本时，
使用 `stdout_text()` 和 `stderr_text()`；需要把非法 UTF-8 字节替换成 `�`
时，使用 `stdout_lossy_text()` 和 `stderr_lossy_text()`。

若实际捕获的 stdout 或 stderr 中含有非法 UTF-8 序列，则 `stdout_text()` /
`stderr_text()` 会分别返回 `Err(str::Utf8Error)`（内部对保留字节执行
`str::from_utf8` 失败），无法得到 `&str`；此时输出仍已完整保留在
`CommandOutput` 中，可改用 `stdout()` / `stderr()` 取得保留的原始字节并自行处理。
配置捕获上限时，若截断位置恰好落在多字节 UTF-8 序列中间，即使进程的完整输出本身
是有效 UTF-8，严格解码也可能失败。

不再需要其余元数据时，可以通过 `into_stdout()` 和 `into_stderr()` 无复制地取走保留字节；
同样，`CommandError::into_output()` 可以从错误中取走保留输出。

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::shell("printf '\\377'"))?;

assert_eq!(output.stdout_lossy_text(), "\u{fffd}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./ci-check.sh

# 检查代码覆盖率
./coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./align-ci.sh`格式化代码，运行`./ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-command](https://github.com/qubit-ltd/rs-command)
