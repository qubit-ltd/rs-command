# Qubit Command 设计说明

[English](design.md) · [用户指南](user_guide.zh_CN.md) · [中文 README](../README.zh_CN.md)

本文记录 `qubit-command` 0.6 的 command runner 契约，面向需要理解进程所有权、I/O
helper、终止流程以及错误保留信息的维护者和调用方。可运行示例仍以用户指南为入口。

## 目的

`CommandRunner` 将外部进程变成一个有边界的应用步骤。runner 从启动到正常结束或终态
错误，一直拥有子进程、stdout/stderr reader 和可选的 stdin writer。进程控制、流采集、
输出上限、取消、超时和诊断脱敏都显式可见。本 crate 不安装信号处理器，不定义 shell
语言，不缓存命令输出，也不会让进程输出自动变得安全。

## 公共契约

- `Command` 描述程序、结构化参数、环境变量和工作目录变更，以及 stdin。`Command::new`
  不调用 shell；`Command::shell` 明确选择类 Unix 平台的 `sh -c` 或 Windows 的 `cmd /C`。
- `CommandRunner` 提供超时、成功退出码、输出捕获、日志、timer 和脱敏策略。`run` 是
  同步入口，`run_with` 接受取消句柄和 tee 文件等单次运行选项。
- `CommandOutput` 携带退出状态、保留的 stdout/stderr 字节、耗时、截断标志和
  `stdout_complete()`/`stderr_complete()`。
- `CommandError` 通过稳定的 `kind()` 分类并可选地携带输出。`output()` 借用保留输出，
  `into_output()` 将其取出；清理失败由 `cleanup_failures()` 提供，不替代主错误。

默认每个流最多捕获 1 MiB，退出码 0 表示成功。`stdin_file` 和 tee 路径必须指向普通
文件；每次运行开始时 tee 会被替换而不是追加。调用 `unbounded_output()` 就意味着调用方
自行接受无界的进程内存输出。

## 所有权状态

runner 按以下状态转移：

1. **已准备：** 校验命令、文件类型、路径冲突和取消状态。输入或 tee 路径冲突、非普通
   文件会在启动子进程及截断 tee 文件之前失败。
2. **已启动：** 子进程和所需 I/O helper 都已创建。子进程、reader 线程、writer 线程、
   取消句柄和 timer 由同一个运行状态拥有。
3. **已观察：** 子进程已退出、超时或观察到取消。runner 仍保留进程状态和 helper 结果，
   直到完成策略处理完毕。
4. **已终结：** 契约允许 join 的 helper 已结束，其结果已合并到 `CommandOutput` 或清理
   诊断。在 Windows 上，helper 取消失败有有界的确认窗口；未确认的 helper 可能脱离
   join，并在管道关闭后结束。

启动前的最终取消检查是线性化点。此前取消返回 `CancelledBeforeStart`，不会创建或截断
tee 文件；此后取消属于运行中取消，并可能保留部分输出。

## 运行事件

正常事件顺序如下：

1. 校验命令、文件类型、路径冲突和取消状态。
2. 启动子进程并配置进程树管理。
3. 启动 stdout/stderr reader 和可选的 stdin writer。
4. 记录启动后的单调时钟时刻，并轮询进程状态、超时和应用取消。
5. 正常退出时 join helper 并组装输出。
6. 超时或取消时终止受管进程树，请求取消 helper，收集可用字节，并确定主错误类别。

超时从子进程启动后开始计时。同步 `run` 阻塞时，timer 仍必须能够推进。取消句柄是
一次性且幂等的；重复调用 `cancel()` 不会产生额外状态迁移。

## 终止

具备进程树管理时，runner 首先请求终止整棵进程树。请求失败后，会检查子进程是否已经
退出，再尝试直接终止子进程。如果仍能确认子进程状态，原始的树终止原因会作为清理失败
保留。等待和 kill 错误会在不丢失更早终止证据的前提下映射；若无法确认最终状态，进程
控制错误仍是主错误。

终止对于后代进程和外部副作用只能尽力而为。runner 不承诺撤销子进程或脱离受管进程树
的后代已经完成的工作。

## I/O 收尾

每个输出 reader 和可选 stdin writer 都拥有取消 token 与 join handle。runner 会先发出
全部取消请求，再等待任一 helper，因此某个请求失败不会阻止其他请求。正常结束时 join
所有 helper 并返回流字节；中断时保留可取得的字节。如果 helper 没有读到管道 EOF，相应
的 `*_complete()` 会为 false。

Unix reader 和 stdin writer 使用非阻塞操作及唤醒通道，因此不会等待逃逸的后代关闭继承
的管道。Windows 使用 `CancelSynchronousIo` 中断同步操作。如果 Windows 取消请求失败，
runner 最多等待 100 ms 来确认 helper 已停止。超过期限后，runner 返回有界结果，并按需
保留 `StdoutCancellation`、`StderrCancellation` 或 `StdinCancellation` 清理失败；对应
helper 可能一直存活到管道关闭。

输出截断与流完整性相互独立。完整流仍可能在内存中被截断，不完整流也可能只包含少量
字节。将保留字节当成完整 transcript 前，调用方必须同时检查这两类元数据。

## 错误优先级

主错误说明运行策略中第一个决定性失败。终止子进程或收尾 helper 时观察到的次级失败按
确定的 helper 顺序放入 `cleanup_failures()`，既保留命令失败原因，也不隐藏清理证据。

准备、线程启动、timer 和进程控制错误可能不携带 `CommandOutput`。超时、取消、截断、
非预期退出、输出读取、tee 写入和最终 stdin 写入错误，在能够组装进程状态、耗时和流状态
时会携带输出。清理失败不会使部分输出变完整，也不表示所有后代都已停止。

## 平台语义

Unix 使用 process group 和非阻塞管道轮询。文件型 stdin 与 tee 文件先使用非阻塞安全
标志打开，从活动句柄检查，再在交给子进程或 tee writer 前恢复阻塞模式；这样路径被替换
为 FIFO 时不会阻塞准备阶段。

Windows 在配置可用时使用 Job Object 管理进程树，管道句柄保持同步 I/O 语义，并通过
`CancelSynchronousIo` 作为正常 helper 中断机制。该调用失败会通过类型化 cancellation
cleanup failure 和上述 100 ms 确认规则暴露出来。确认规则保证返回有界，不保证操作系统
立即关闭管道。

其他平台仍会依据活动句柄检查普通文件，但没有可移植的方法保证所有设备命名空间的打开
都能及时返回。损坏的远程或 FUSE 文件系统仍可能让操作系统调用停滞，这不属于命令超时
契约。

## 测试接缝

runner 接受注入的 timer 和 clock，测试可以推进时间而无需真实等待超时。进程控制包装、
文件准备、输出 reader、tee writer、取消 token 和清理失败映射都在各自接缝测试。集成测试
还覆盖真实命令、shell 选择、非 UTF-8 值、普通文件拒绝、进程树清理、取消、超时、截断、
tee 输出和脱敏诊断。

## 覆盖率门禁

生命周期变更必须继续覆盖正常退出、超时、启动前取消、运行中取消、helper 取消失败、
部分输出、清理错误优先级，以及 Unix/Windows 差异路径。文档示例通过 doctest 或仓库
Markdown 检查器编译。发布包验证必须包含两份 README、两份用户指南和两份设计文档，且不
得依赖 Cargo 归档中排除的文件。

## 已知限制

- 本库无法验证目标程序参数或 shell 脚本的语义；输入校验由调用方负责。
- 进程树终止和 helper 中断依赖操作系统能力。后代可能保持继承管道打开，Windows helper
  也可能超过有界确认窗口后仍存活。
- 即使诊断已经脱敏，捕获输出和 tee 文件仍是原始进程数据；保留和访问策略由调用方决定。
- 超时不包括准备和启动，也不限制不可信或远程文件系统上的任意内核操作。
- 后续公共生命周期变更必须同步错误契约、流完整性语义、双语指南和本文档。
