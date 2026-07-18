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
- 超时时基于 Unix process group 和 Windows Job Object 尝试终止进程树。
- 默认以 UTF-8 文本读取 stdout 和 stderr，同时提供原始字节访问方法。
- 支持按流限制内存捕获字节数，并把完整输出流式写入文件。
- 在截断任何 tee 文件前检查 stdin、stdout 和 stderr 的文件冲突。
- 日志和诊断里的命令文本会对敏感 argv、显式环境变量覆盖、shell
  脚本体以及调用方追加的敏感字段做脱敏展示。
- 使用明确错误类型表示进程启动失败、超时、输出读取失败和非预期退出码。

## 超时行为

`CommandRunner::new()` 默认不限制执行时间。需要约束命令运行时长时，请显式调用
`timeout(Duration)`；如果希望在 builder 链中明确表达不设超时，可以调用
`without_timeout()`。

设置超时后，runner 会尝试终止整个进程树：Unix 平台把命令放入新的
process group，Windows 平台把命令放入 Job Object。

超时测量和休眠使用可注入的 `qubit-clock` timer，因此单元测试可以用手动单调时钟
驱动超时逻辑。未设置超时时，runner 会直接等待进程结束，不进行轮询。

## 大输出

默认情况下 stdout 和 stderr 的内存捕获不设字节上限。如果命令可能输出大量日志，
可以同时设置捕获上限和 tee 文件：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .max_output_bytes(64 * 1024)
    .tee_stdout_to_file("stdout.log")
    .tee_stderr_to_file("stderr.log")
    .run(Command::new("cargo").arg("test"))?;

if output.stdout_truncated() {
    eprintln!("stdout was truncated in memory; see stdout.log for the full stream");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 快速开始

```rust
use std::time::Duration;

use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .timeout(Duration::from_secs(10))
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

## 诊断脱敏

Runner 日志、`CommandError::command()` 和 `Command` 的 `Debug` 输出都会通过
`qubit-sanitize` 生成脱敏命令文本。类似 `--password secret`、
`--access-token=...`、`OPENAI_API_KEY=...` 的结构化 argv 值会被遮蔽；显式设置的
环境变量覆盖也只展示脱敏后的 `KEY=value`。`Command::shell` 的脚本体不做 shell
语法解析，统一作为不透明脚本显示为 `<shell command>`。

当默认敏感字段不够时，可以在 runner 上追加业务字段：

```rust
use qubit_command::{Command, CommandRunner};
use qubit_sanitize::SensitivityLevel;

let error = CommandRunner::new()
    .sensitive_field("tenant_option", SensitivityLevel::Secret)
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

Runner 上追加的字段只影响 runner 日志和 `CommandError::command()`。
独立的 `Command` `Debug` 输出没有 runner 上下文，只使用内置默认字段。对于确认过的
误报，runner 可以调用 `exclude_sensitive_field` 或 `exclude_sensitive_fields` 排除默认
字段。这会让匹配的 argv 或环境变量值原样出现在 runner 日志和
`CommandError::command()` 中，因此每个排除项都应经过安全审阅。

`CommandOutput` 的 `Debug` 输出会遮盖两个捕获流，只报告字节数、截断标志、退出状态和
耗时。捕获到的 stdout/stderr 字节、显式访问方法以及 tee 文件仍然是进程原始输出。
如果命令输出本身可能包含敏感信息，请配置捕获上限，并在调用方按业务语义过滤。

## 输出文本

`stdout()` 和 `stderr()` 返回保留下来的原始字节。需要严格 UTF-8 文本时，
使用 `stdout_text()` 和 `stderr_text()`；需要把非法 UTF-8 字节替换成 `�`
时，使用 `stdout_lossy_text()` 和 `stderr_lossy_text()`。

若实际捕获的 stdout 或 stderr 中含有非法 UTF-8 序列，则 `stdout_text()` /
`stderr_text()` 会分别返回 `Err(str::Utf8Error)`（内部对保留字节执行
`str::from_utf8` 失败），无法得到 `&str`；此时输出仍已完整保留在
`CommandOutput` 中，可改用 `stdout()` / `stderr()` 取得原始字节并自行处理。

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
