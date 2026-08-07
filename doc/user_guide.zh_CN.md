# Qubit Command 用户手册

[English user guide](user_guide.md) · [中文 README](../README.zh_CN.md) · [API 文档](https://docs.rs/qubit-command)

本手册针对 `qubit-command` 0.6.0，面向需要从 Rust 应用运行外部程序，并明确处理进程生命周期、输出大小、取消和诊断边界的开发者。

## 本 crate 解决什么问题

一个外部命令可以划分为四个使用边界：

| 类型 | 职责 |
| --- | --- |
| `Command` | 描述哪个程序接收哪些参数、环境变量、工作目录和 stdin。 |
| `CommandRunner` | 定义如何启动、等待、取消、记录日志和限制进程。 |
| `CommandOutput` | 携带观察到的退出状态、保留的输出、流完整性和耗时。 |
| `CommandError` | 解释准备、执行、采集或成功策略为何没有正常完成。 |

`CommandCancellation` 用于把 runner 接入应用已经拥有的关闭策略。本 crate 刻意不安装全局信号处理器。

## 贯穿场景：运行仓库检查

目标很明确：运行 `git status --short`，把 stdout 作为 UTF-8 返回；如果可执行文件无法启动或退出状态异常，则保留一个有类型的错误。

### 安装

使用 Rust 1.94 或更高版本，在应用中添加：

```toml
[dependencies]
qubit-command = "0.6"
```

### 构造并运行命令

普通命令使用结构化参数。这样参数边界明确，也不会调用 shell：

```rust
use qubit_command::{Command, CommandCancellation, CommandRunOptions, CommandRunner};

fn repository_status() -> Result<String, Box<dyn std::error::Error>> {
    let output = CommandRunner::new(std::time::Duration::from_secs(10))
        .run(Command::new("git").args(&["status", "--short"]))?;

    Ok(output.stdout_text()?.to_owned())
}
```

`CommandRunner::new(std::time::Duration::from_secs(10))` 默认使用十秒超时，把退出码 `0` 视为成功，并在内存中为每个输出流最多保留 1 MiB。`run` 会同步等待命令成功，或返回 `CommandError`。

### 判断输出含义

`stdout()` 和 `stderr()` 返回保留的原始字节。只有确实要求严格 UTF-8 时才使用 `stdout_text()` 或 `stderr_text()`；如果希望把非法字节替换为 `�`，使用 `stdout_lossy_text()` 或 `stderr_lossy_text()`。

```rust
use qubit_command::{Command, CommandRunOptions, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::new("printf").arg("hello"))?;

assert_eq!(output.stdout_text()?, "hello");
# Ok::<(), Box<dyn std::error::Error>>(())
```

严格解码失败时，原始字节仍可通过 `stdout()` 或 `stderr()` 取得。捕获上限也可能在多字节序列中间截断，因此截断与 UTF-8 有效性是两个独立决策。

## 核心工作流

### 优先使用结构化命令

```rust
let command = Command::new("git")
    .args(&["status", "--short"])
    .working_directory("/workspace/project")
    .env("LC_ALL", "C");
```

参数会直接传递给目标程序，不进行 shell 引号处理或变量展开。目标程序仍会按照自己的规则解析选项。如果值可能以 `-` 开头，应遵循目标程序的参数规则，通常是把它放在 `--` 之后。

对于非 UTF-8 的程序名或参数，使用 `new_os`、`arg_os`、`args_os`、`env_os` 或 `sensitive_arg_os`。

### 只在明确需要时使用 shell

```rust
let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::shell("printf hello | tr a-z A-Z"))?;
assert_eq!(output.stdout_text()?, "HELLO");
# Ok::<(), Box<dyn std::error::Error>>(())
```

类 Unix 平台上的 `Command::shell` 执行 `sh -c`，Windows 上执行 `cmd /C`。shell 脚本体在诊断中视为不透明 secret。shell 的展开、重定向和管道由调用方负责，包括输入校验。

### 配置输入与环境

`Command` 可以继承 stdin、使用空 stdin、提供字节或从文件读取；也可以继承环境、添加或覆盖变量、删除变量，或者先清空继承环境再应用显式值：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10)).run(
    Command::new("cat")
        .stdin_bytes("input\n")
        .env("LANG", "C"),
)?;

assert_eq!(output.stdout_text()?, "input\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`stdin_file` 只接受普通文件。目录、FIFO、设备、套接字及其他特殊文件会在启动子进程前被拒绝。

### 定义成功退出码

默认只有退出码 `0` 表示成功。如果工具文档说明另一个状态也代表成功，应明确配置：

```rust
let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .success_exit_codes(&[0, 2])
    .run(Command::new("tool"))?;
# let _ = output;
```

不在配置列表中的退出状态会返回 `CommandError::UnexpectedExit`，并保留捕获到的输出。

## 超时与取消

### 超时

每个 `CommandRunner` 实例都会用一个显式 timeout 构造。
以下示例使用十秒超时。
它从子进程启动后开始计时，因此准备和启动耗时不计入该时长。

```rust
use std::time::Duration;
use qubit_command::{Command, CommandRunner};

let result = CommandRunner::new(std::time::Duration::from_secs(10))
    .run(Command::new("long-running-tool"));
```

达到 deadline 后，runner 会尝试终止受管进程树，收集可用输出，等待 I/O 辅助线程结束，并返回 `CommandError::TimedOut`。超时错误可能只包含部分输出。

只有在明确需要无限等待时才使用 `without_timeout()`。如果配置了取消句柄，即使没有超时，runner 仍会轮询 timer 并管理进程树。

### 使用应用自己的取消策略

创建一个取消句柄，将其 clone 后交给 runner，并从应用已有的关闭策略中调用 `cancel()`：

```rust
use qubit_command::{Command, CommandCancellation, CommandRunner};

let cancellation = CommandCancellation::new();
let runner = CommandRunner::new(std::time::Duration::from_secs(10));
let result = runner.run_with(
    Command::new("long-running-tool"),
    CommandRunOptions::new().cancellation(cancellation.clone()),
);

// 在应用的关闭或终端信号策略中调用：
cancellation.cancel();

let result = runner.run_with(
    Command::new("long-running-tool"),
    CommandRunOptions::new().cancellation(cancellation.clone()),
);
```

如果在准备开始前观察到取消请求，结果是 `CommandError::CancelledBeforeStart`。否则 runner 会终止受管进程树，并在可用时通过 `CommandError::Cancelled` 保留输出。句柄是一次性的；多次调用 `cancel()` 不会产生额外效果。

启用超时或取消的等待时，timer 必须能在 `run()` 同步阻塞调用方线程时继续推进。Tokio timer 不应依赖只能由同一阻塞线程驱动的 current-thread runtime。

## 有界输出与大输出

默认每个流的上限是 `DEFAULT_MAX_OUTPUT_BYTES_PER_STREAM`，当前为 1 MiB。成功命令的保留输出被截断时，除非关闭该策略，否则会返回 `CommandError::OutputTruncated`。

对于大量日志，保持内存有界，并把每个流 tee 到文件：

```rust
use qubit_command::{Command, CommandRunner};

let output = CommandRunner::new(std::time::Duration::from_secs(10))
    .max_output_bytes(64 * 1024)
    .fail_on_output_truncation(false)
    .run_with(
        Command::new("cargo").arg("test"),
        CommandRunOptions::new()
            .tee_stdout_to_file("stdout.log")
            .tee_stderr_to_file("stderr.log"),
    )?;

if output.stdout_truncated() {
    eprintln!("stdout was truncated in memory; see stdout.log for the full stream");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`bounded_output(max_bytes)` 同时修改两个流的上限，并保留默认的截断失败策略。`max_stdout_bytes` 和 `max_stderr_bytes` 可以分别配置两个流。`unbounded_output()` 会移除两个上限，只有在确认可信命令的输出有限时才应使用。

超时、取消、非预期退出和输出截断错误都可能通过 `CommandError::output()` 暴露 `CommandOutput`，也可以通过 `CommandError::into_output()` 将其取出。对于超时和取消，请先检查 `stdout_complete()` 与 `stderr_complete()`，再判断保留字节是否代表完整流。

tee 路径必须是普通文件。runner 会在截断任何 tee 文件前检查 stdin/tee 以及 stdout tee/stderr tee 之间的冲突。
每次运行都会截断并替换 tee 文件，不会追加。复制 `CommandRunOptions` 会复制已配置的路径；如果并发运行需要分别保留日志，必须使用不同路径。

## 诊断与脱敏

Runner 日志、`CommandError::command()` 和 `Command` 的 debug 输出会遮盖敏感结构化参数、显式环境变量覆盖、shell 内容和已配置路径。对于客户文件路径等位置参数，使用 `sensitive_arg` 或 `sensitive_arg_os`：

```rust
let command = Command::new("uploader")
    .arg("--file")
    .sensitive_arg("customer-report.csv");
```

原值会不变地传给子进程，诊断渲染时则显示掩码。`CommandRunner::new(std::time::Duration::from_secs(10))` 会取得进程级默认脱敏策略的快照。如果要构造 runner 专用策略，可以通过 `diagnostic_redaction_policy` 注入完整不可变策略；策略类型属于 `qubit-redact`，因此应用构造它时必须直接依赖该 crate。

`allow_exact` 和 `allow_suffix` 规则可能让值出现在诊断中，应先审阅其确切披露边界。`CommandOutput` 的 debug 输出会遮盖捕获流，只展示元数据；显式字节访问器和 tee 文件仍是原始进程输出。

生命周期记录使用 `debug` 级别。`disable_logging(true)` 会抑制这些记录；无法通过 `CommandError` 返回的清理失败仍可能以 `error` 级别记录。

## 错误与诊断

`CommandError` 是 non-exhaustive 枚举，因此下游匹配必须保留通配分支。重要类别包括：

| 类别 | 示例 | 下一步诊断 |
| --- | --- | --- |
| 准备 | `OpenInputFailed`、`NonRegularInputFile`、`InputOutputConflict`、`OutputFilesConflict` | 检查路径、文件类型以及输入/输出路径是否不同。 |
| 进程控制 | `SpawnFailed`、`WaitFailed`、`KillFailed`、`CancelFailed` | 检查可执行文件、权限、平台进程控制能力和源 I/O 错误。 |
| 流 I/O | `ReadOutputFailed`、`WriteInputFailed`、`OpenOutputFailed`、`WriteOutputFailed` | 检查对应流、文件访问权限以及错误附带的保留输出。 |
| 时间 | `TimeFailed` | 检查 timer 与 clock 是否使用有效的单调时间域，并能在调用方阻塞时推进。 |
| 策略结果 | `UnexpectedExit`、`OutputTruncated`、`TimedOut`、`Cancelled`、`CancelledBeforeStart` | 检查状态、配置策略、保留输出和流完整性标志。 |

处理带输出的策略错误时，可以这样检查：

```rust
use qubit_command::{Command, CommandError, CommandRunner};

match CommandRunner::new(std::time::Duration::from_secs(10)).run(Command::new("tool")) {
    Ok(output) => println!("{}", output.stdout_lossy_text()),
    Err(error) => {
        eprintln!("{}", error);
        if let Some(output) = error.output() {
            eprintln!("stdout complete: {}", output.stdout_complete());
            eprintln!("stderr complete: {}", output.stderr_complete());
        }
        if matches!(error, CommandError::TimedOut { .. }) {
            eprintln!("the process exceeded its timeout");
        }
    }
}
```

## 排障

### 找不到命令

`SpawnFailed` 表示操作系统无法启动请求的程序。确认程序名或路径、进程环境和工作目录；如果可执行文件路径不是有效 UTF-8，使用 `Command::new_os`。

### 命令退出状态异常

读取 `CommandError::UnexpectedExit`，检查 `exit_code`、`expected`、stdout 和 stderr，再判断该状态对当前业务是否确实代表成功。不要静默地把所有非零退出码当成成功。

### 命令超时或被取消

可以使用保留输出诊断，但必须检查两个完整性标志。即使清理已经返回，后代进程或阻塞 I/O 仍可能导致流不完整。

### 输出被截断

保持有界设置并使用 tee 文件保存完整日志，或提高流上限。只有业务明确接受内存中的部分输出时，才关闭截断失败策略。不要对不可信或开放式命令使用 `unbounded_output()`。

### 文本解码失败

二进制输出使用原始 `stdout()`/`stderr()`，或使用严格访问器后由调用方选择其他 codec。如果允许替换字符，使用有损访问器。截断可能切断有效 UTF-8 序列。

### 文件配置被拒绝

确认 stdin 和 tee 路径都是普通文件，不是目录或特殊文件；同时确认任意两个配置路径都不指向同一文件。这些检查发生在子进程启动前，也发生在 tee 文件截断前。

### 诊断信息过多或过少

对调用方已知的 secret 使用 `sensitive_arg`，并仔细审阅脱敏放行规则。显式输出访问器和 tee 文件包含原始进程输出；诊断脱敏不会过滤这些数据。

## 限制与最佳实践

- 优先使用带显式参数的 `Command::new`。只有 shell 行为本身是需求时才使用 `Command::shell`。
- 除非命令行为确实要求改变，否则保留默认超时和有界捕获。
- 把超时和取消输出视为可能不完整。
- 大日志使用 tee 文件，不要轻易使用无界内存捕获。
- 将 stdin 和 tee 路径配置为互不相同的普通文件。
- 从应用已有的关闭策略配置取消；本 crate 不安装全局信号处理器。
- 启用超时或取消时，选择能独立于同步调用方持续推进的 timer 后端。
- 即使诊断已脱敏，也要把捕获输出和 tee 文件当作不可信的进程数据处理。

## 相关资源

- [中文 README](../README.zh_CN.md)
- [English README](../README.md)
- [English user guide](user_guide.md)
- [API 文档](https://docs.rs/qubit-command)
- [命令 runner 的 I/O 生命周期设计](command-runner-io-lifecycle-design.md)
