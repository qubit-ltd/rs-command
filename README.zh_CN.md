# Qubit Command

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-command.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-command)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-command/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-command?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-command.svg?color=blue)](https://crates.io/crates/qubit-command)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Documentation](https://img.shields.io/badge/docs-English-blue.svg)](README.md)

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
- 使用明确错误类型表示进程启动失败、超时、输出读取失败和非预期退出码。

## 超时行为

`CommandRunner::new()` 默认不限制执行时间。需要约束命令运行时长时，请显式调用
`timeout(Duration)`；如果希望在 builder 链中明确表达不设超时，可以调用
`without_timeout()`。

设置超时后，runner 会尝试终止整个进程树：Unix 平台把命令放入新的
process group，Windows 平台把命令放入 Job Object。

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

println!("{}", output.stdout()?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Shell 命令

优先使用结构化命令：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::new("printf").arg("hello"))?;

assert_eq!(output.stdout()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

只有在明确需要 shell 解析、重定向、变量展开或管道时，才使用
`Command::shell`：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .run(Command::shell("printf hello | tr a-z A-Z"))?;

assert_eq!(output.stdout()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 输出文本

`stdout()` 和 `stderr()` 默认返回 UTF-8 文本。如果命令可能输出任意字节，
使用 `stdout_bytes()` 和 `stderr_bytes()` 获取原始输出。需要把非法 UTF-8
字节替换成 `�` 时，在 runner 上启用 lossy 输出：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new()
    .lossy_output(true)
    .run(Command::shell("printf '\\377'"))?;

assert_eq!(output.stdout()?, "\u{fffd}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 测试

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
