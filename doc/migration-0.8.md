# Migrating to qubit-command 0.8

Version 0.8 deliberately changes error precedence. Replace matches on
`CommandErrorKind::KillFailed` or `CancelFailed` (and corresponding
`CommandErrorReason` variants) with `TimedOut` or `Cancelled`, then inspect
`cleanup_failures()`. The old variants are removed rather than deprecated.

```rust
use qubit_command::{CommandCleanupFailure, CommandError, CommandErrorKind};

fn inspect(error: &CommandError) {
    match error.kind() {
        CommandErrorKind::TimedOut => eprintln!("command timed out"),
        CommandErrorKind::Cancelled => eprintln!("command was cancelled"),
        _ => eprintln!("command failed"),
    }
    for failure in error.cleanup_failures() {
        match failure {
            CommandCleanupFailure::ProcessTreeTermination { .. }
            | CommandCleanupFailure::ChildTermination { .. } => {
                eprintln!("process termination was not confirmed");
            }
            CommandCleanupFailure::Time { .. } => {
                eprintln!("elapsed time could not be measured during cleanup");
            }
            _ => eprintln!("another cleanup operation failed"),
        }
    }
}
```

`process_tree_source()` and `child_source()` remain convenient typed accessors.
`Error::source()` still describes the primary failure; cleanup sources are
available separately. Partial output survives helper failures when status and
elapsed time are reliable. If they are unavailable, `output()` may be `None`.
Never assume that a timeout means the operating system stopped every process.

Startup failures now explicitly collect cleanup evidence. A rejected tree kill
followed by a rejected direct-child kill does not cause an unbounded wait for an
unknown child status. A successful kill request may still require OS reaping;
remote filesystem stalls remain outside the timeout contract.

## 中文迁移要点

将旧 `KillFailed` / `CancelFailed` 分支改为 `TimedOut` / `Cancelled`，并检查
`cleanup_failures()` 中的进程终止错误。收尾时钟错误新增 `Time` 清理项。已有的
`process_tree_source()`、`child_source()` 仍可使用；主错误 source 不代替清理错误列表。
有可靠退出状态和耗时时会保留部分输出，否则允许 `output()` 为空。终止失败不表示
子进程已经退出，普通文件 I/O 和操作系统回收仍不提供硬超时承诺。
