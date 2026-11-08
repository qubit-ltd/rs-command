# Qubit Command

[![Rust CI](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-command/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-command/coverage-badge.json)](https://qubit-ltd.github.io/rs-command/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-command.svg?color=blue)](https://crates.io/crates/qubit-command)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

Qubit Command 是一个 Rust 外部进程运行库。当应用不仅需要启动进程，还需要限制输出捕获、定义超时或取消策略、清理进程树，并在错误诊断中隐藏敏感信息时，它可以把这些边界统一成结构化 API。它适合构建工具、服务后台以及其他必须把外部进程作为受控步骤执行的应用。

本文针对 `qubit-command` 0.11 版本，下文说明 crate 与工具链要求。

## 安装

crate 名称为 `qubit-command`，要求 Rust 1.94 或更高版本：

```toml
[dependencies]
qubit-command = "0.11"
```

## 快速开始

假设服务需要运行一次仓库检查，并把命令输出放入结果中。使用程序和参数值构造结构化命令，用默认策略运行，确认命令成功后再解码 stdout：

<!-- doc-example:quick-start:compile -->
```rust
use qubit_command::{Command, CommandRunner};

use std::time::Duration;

fn repository_status() -> Result<String, Box<dyn std::error::Error>> {
    let output = CommandRunner::new(Duration::from_secs(10))
        .run(Command::new("git").args(&["status", "--short"]))?;

    Ok(output.stdout_text()?.to_owned())
}
```

结构化形式不会进行 shell 解析。如果确实需要 shell 管道或重定向，请显式使用 `Command::shell(...)`，并由调用方负责验证传入的 shell 命令行。

## 为什么需要这个项目

启动子进程很简单；但当进程挂起、输出过大、取消后仍有子孙进程持有管道、退出状态不符合预期，或诊断信息可能包含 secret 时，应用必须自行定义清晰的处理策略。Qubit Command 将这些决策集中在 `CommandRunner`，并通过 `CommandOutput` 和 `CommandError` 暴露可观察结果。

当调用方需要对外部进程使用可重复的运行策略时，这个库可以减少重复的生命周期处理。它不替代目标程序自己的参数解析，不提供 shell 语言抽象，也不会安装全局信号处理器。

## 核心能力与边界

- `Command` 描述程序、结构化参数、可选的 shell 执行方式、工作目录和环境变量覆盖，以及 stdin 配置。
- `CommandRunner` 应用超时、取消、成功退出码、日志、输出捕获、tee 文件和诊断脱敏策略。
- `CommandOutput` 提供退出状态、原始 stdout/stderr 字节、严格或有损 UTF-8 视图、耗时、截断标志和流完整性标志。
- `CommandError` 区分准备、启动、等待、输出、超时、取消、截断和非预期退出错误。当进程状态和流状态可以可靠组装时，超时、取消、截断、非预期退出、tee 写入、输出读取以及最终 stdin 写入错误会保留 `CommandOutput`；准备、线程启动、时钟和进程控制错误可能不携带输出。
- `CommandCancellation` 是供应用自己的关闭或终端信号策略使用的一次性句柄。本 crate 不安装信号处理器。
- 配置超时、取消或总输出预算时，runner 会通过 Unix process group 或 Windows Job Object 尝试终止进程树。
- 在 Windows 上，`CancelSynchronousIo` 成功只表示取消请求成功，并不能证明 helper 已停止。随后 runner 会 join 该 helper；这个 join 不受 100 ms 截止时间限制。runner 会在发出取消请求前统一设定一个共享的 100 ms helper 确认截止时间，只有请求失败时才会检查该时间，且不会为每个 helper 重置；它也不是整个调用的硬期限。若该截止时间耗尽，为保证返回有界，结果会保留 cancellation cleanup failure；受影响的 helper 可能一直存活到管道关闭。当前核查状态见[Windows 取消验证](doc/rc-002-verification.md)。
- 默认每个输出流最多在内存中保留 1 MiB。单流内存保留上限本身不会停止排空或 tee 写入；总输出预算、超时、取消或管道读取失败可能中断收集。tee 写入失败可能在读取继续时使 tee 不完整；tee flush 失败则可能在流已排空后使 tee 不完整。
- 可选的 stdout/stderr 总预算会停止超量输出的命令，与每流内存保留上限独立。发生 `OutputLimitExceeded` 时，通过 `CommandError::captured_output()` 检查部分字节；`output()` 为 `None`。详见[中文用户手册](doc/user_guide.zh_CN.md)。
- 命令诊断和生命周期日志会遮盖显式标记的敏感参数、环境变量、已配置路径，以及通过 `Command::shell` 传入的整段脚本。整段脚本的自动隐藏范围仅限该生成形式和识别出的 shell 参数形式；手工构造的 `bash -c` 或 `powershell -Command` 等脚本若含敏感内容，需使用 `sensitive_arg` 或 `sensitive_arg_os`。捕获到的进程输出与 tee 文件仍是原始输出，需要由调用方自行处理。

重要边界：

- `Command::new` 避免了 shell 解析，但目标可执行文件仍会按自身规则解析选项。
- `Command::shell` 在类 Unix 平台使用 `sh -c`，在 Windows 使用 `cmd /C`；它不承诺一种跨平台的 shell 语言。
- 手工构造解释器命令时，应显式标记含敏感内容的脚本参数；自动整段脚本脱敏不覆盖所有解释器形式。
- `unbounded_output()` 会移除内存捕获上限，只有在确认命令输出有限且可接受时才应使用。
- 超时和取消结果可能只包含部分输出。在把保留字节当作完整流前，请检查 `stdout_complete()` 和 `stderr_complete()`。
- `stdin_file`、`tee_stdout_to_file` 和 `tee_stderr_to_file` 只接受普通文件。输入文件与输出文件冲突时，会在截断 tee 文件前拒绝执行。
- 在 Unix 上，文件型 stdin 和 stdout/stderr tee 目标使用非阻塞安全标志打开，依据已打开句柄验证，并在使用前恢复阻塞模式，从而避免路径被替换为 FIFO 后阻塞命令准备。其他平台会验证已打开句柄，但无法以可移植方式保证所有设备命名空间的打开操作及时返回，因此只应传入可信的普通文件路径。
- tee 文件会在每次运行开始时替换（而不是追加）。`CommandRunOptions::clone()` 会复制 tee 路径；如果并发运行需要分别保留日志，必须提供不同路径。

## 延伸阅读

- [English user guide](doc/user_guide.md)
- [中文用户手册](doc/user_guide.zh_CN.md)
- [docs.rs API 文档](https://docs.rs/qubit-command)
- [命令 runner 的 I/O 生命周期设计](doc/design.md)
- [English README](README.md)

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./.infra/bin/ci-check.sh

# 检查代码覆盖率
./.infra/bin/coverage.sh
```

文档示例 harness 会先根据根目录的 `Cargo.lock` 执行 `cargo fetch --locked`，再以离线
模式检查依赖并构建、运行示例。首次运行时，fetch 可能需要联网。使用
`CARGO_BUILD_TARGET` 或 Cargo 配置可让 metadata 与测试构建采用相同目标；若要运行
其他目标的测试，还需要已配置的 Cargo runner。

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./.infra/bin/align-ci.sh`格式化代码，运行`./.infra/bin/ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-command](https://github.com/qubit-ltd/rs-command)
