# Qubit Command

[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Documentation](https://img.shields.io/badge/docs-English-blue.svg)](README.md)

面向 Qubit Rust 项目的命令行进程运行工具库。

## 概览

Qubit Command 提供一个小而明确的结构化 API，用于运行外部程序、捕获
stdout/stderr、控制超时，并用清晰的错误类型报告命令执行失败。

本 crate 刻意独立于 `qubit-concurrent`：运行外部命令属于进程管理，不是通用任务执行策略。

## 功能

- 使用 program + args 的结构化命令执行方式。
- 在确实需要 shell 解析时，提供显式 shell 命令支持。
- 支持配置超时、工作目录、环境变量和成功退出码。
- 以原始字节捕获 stdout 和 stderr，并提供 UTF-8 辅助方法。
- 使用明确错误类型表示进程启动失败、超时、输出读取失败和非预期退出码。

## 快速开始

```rust
use std::time::Duration;

use qubit_command::{CommandRunner, CommandSpec};

let output = CommandRunner::new()
    .timeout(Duration::from_secs(10))
    .run(CommandSpec::new("git").args(&["status", "--short"]))?;

println!("{}", output.stdout_utf8()?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Shell 命令

优先使用结构化命令：

```rust
use qubit_command::{CommandRunner, CommandSpec};

let output = CommandRunner::new()
    .run(CommandSpec::new("printf").arg("hello"))?;

assert_eq!(output.stdout_utf8()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

只有在明确需要 shell 解析、重定向、变量展开或管道时，才使用
`CommandSpec::shell`：

```rust
use qubit_command::{CommandRunner, CommandSpec};

let output = CommandRunner::new()
    .run(CommandSpec::shell("printf hello | tr a-z A-Z"))?;

assert_eq!(output.stdout_utf8()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 测试

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
