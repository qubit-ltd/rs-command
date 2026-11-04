# Qubit Command 设计说明

[English](design.md) · [用户指南](user_guide.zh_CN.md) · [中文 README](../README.zh_CN.md)

本文记录当前 `qubit-command` 的 command runner 契约，面向需要理解进程所有权、I/O
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
  `into_output()` 将其取出。`captured_output()` 借用捕获字节，
  `into_captured_output()` 独立移出字节并保留流标记；两个载体同时存在时优先选择
  `CommandOutput`。消费式方法也会消费错误诊断。清理失败由 `cleanup_failures()` 提供，
  不替代主错误。

默认每个流最多捕获 1 MiB，退出码 0 表示成功。`stdin_file` 和 tee 路径必须指向普通
文件；每次运行开始时 tee 会被替换而不是追加。调用 `unbounded_output()` 就意味着调用方
自行接受无界的进程内存输出。

通过 `stdin_file`、`tee_stdout_to_file` 和 `tee_stderr_to_file` 配置的相对路径，以父进程
当前目录为基准打开。command 或 runner 的工作目录设置只改变子进程目录。若调用方希望访问
子进程目录下的文件，必须在配置前将路径显式拼接到指定目录。

错误格式化使用受控 source 摘要：I/O source 仅显示其 `ErrorKind` 和可选的原始 OS 错误
编号，时间 source 详情则省略。原始 source 对象及 `Error::source()` 链仍可访问，其中可能
包含任意文本。调用方若格式化完整原始错误链，应自行负责这些文本的处理。

`max_total_output_bytes(limit)` 可选设置 stdout 与 stderr 共享的硬预算。即使没有配置超时或
取消策略，它也会启用进程树管理。reader 总共最多观察 `limit + 1` 个字节；额外的一个字节
作为哨兵证明预算已超限。每流保留上限仍可使部分字节不进入内存，但这些字节仍计入共享预算，
也仍会写入已配置的 tee。若观察到哨兵，它会计入实际观察量。

预算超限返回 `OutputLimitExceeded`，不会伪造自然进程退出状态。部分字节可从
`CommandError::captured_output()` 读取，此时 `CommandError::output()` 为 `None`。其他错误只有在
收尾成功组装 `CommandOutput` 时才会有 `output()`；若无法组装自然状态、耗时或完整 helper
结果，`captured_output()` 仍可能提供部分数据。捕获访问器暴露原始进程字节，调用方需自行
负责敏感输出的处理。

调用 `CommandError::into_captured_output()` 可将保留字节作为独立值移出，包括仅有预算错误
载体的情况。该方法保留流完整性和截断标记，不会克隆缓冲区；原始进程输出可能包含敏感数据，
调用方仍需负责安全处理。

## 所有权状态

runner 按以下状态转移：

1. **已准备：** 校验命令、文件类型、路径冲突和取消状态。输入或 tee 路径冲突、非普通
   文件会在启动子进程及截断 tee 文件之前失败。
2. **已启动：** 子进程和所需 I/O helper 都已创建。子进程、reader 线程、writer 线程、
   取消句柄和 timer 由同一个运行状态拥有。
3. **已观察：** 子进程已退出、超时或观察到取消。runner 仍保留进程状态和 helper 结果，
   直到完成策略处理完毕。
4. **已终结：** 契约允许 join 的 helper 已结束，其结果已合并到 `CommandOutput` 或清理
   诊断。在 Windows 上，runner 会在发出 helper 取消请求前统一设定一个共享确认截止时间，
   只有取消请求失败时才会检查，且不会为每个 helper 重置。如果在该截止时间前确认 helper
   已结束，后续 join 不受该时间限制；未确认的 helper 可能脱离 join，并在管道关闭后结束。

启动前的最终取消检查是线性化点。此前取消返回 `CancelledBeforeStart`，不会创建或截断
tee 文件；此后取消属于运行中取消，并可能保留部分输出。

## 运行事件

正常事件顺序如下：

1. 校验命令、文件类型、路径冲突和取消状态。
2. 启动子进程并配置进程树管理。
3. 记录启动后的单调时钟时刻。
4. 启动可选的 stdin writer 和 stdout/stderr reader，然后轮询进程状态、超时、应用取消和总输出预算状态。
5. 正常退出时 join helper 并组装输出。
6. 超时或取消时终止受管进程树，请求取消 helper，收集可用字节，并确定主错误类别。

超时在子进程启动后、I/O helper 启动前开始计时；helper 初始化时间计入超时，命令准备和
spawn 时间不计入。超时、取消和总输出预算监控都使用基于 timer 的轮询；即使只配置总输出
预算也如此，因此同步 `run` 阻塞时 timer 仍必须独立推进。字节预算不是墙钟硬期限，也不限制
准备、spawn 或任意文件系统操作。取消句柄是一次性且幂等的；重复调用 `cancel()` 不会产生
额外状态迁移。

## 终止

具备进程树管理时，runner 首先请求终止整棵进程树。请求失败后，会检查子进程是否已经
退出，再尝试直接终止子进程。如果仍能确认子进程状态，原始的树终止原因会作为清理失败
保留。等待和 kill 错误会在不丢失更早终止证据的前提下映射；若无法确认最终状态，仍保留原停止原因，进程
控制失败通过清理信息返回。

终止对于后代进程和外部副作用只能尽力而为。runner 不承诺撤销子进程或脱离受管进程树
的后代已经完成的工作。

启动错误通过同一终止策略显式收尾。两次 kill 均失败且状态未知时，不进入阻塞 wait。
成功提交 kill 后的回收等待和普通文件 I/O 不属于硬超时承诺。

## I/O 收尾

每个输出 reader 和可选 stdin writer 都拥有取消 token 与 join handle。runner 会先发出
全部取消请求，再等待任一 helper，因此某个请求失败不会阻止其他请求。正常结束时 join
所有 helper 并返回流字节；中断时保留可取得的字节。如果 helper 没有读到管道 EOF，相应
的 `*_complete()` 会为 false。

Unix reader 和 stdin writer 使用非阻塞操作及唤醒通道，因此不会等待逃逸的后代关闭继承
的管道。Windows 使用 `CancelSynchronousIo` 中断同步操作。调用成功只表示取消请求成功，
并不能证明 helper 已停止。随后 runner 会 join 该 helper；这个 join 不受 100 ms 截止时间限制。
runner 会在发出取消请求前统一设定一个共享的 100 ms helper 确认截止时间，只有请求失败时才会
检查该时间，且不会为每个 helper 重置；它也不是整个调用的硬期限。超过该截止时间后，runner 返回有界结果，并按需
保留 `StdoutCancellation`、`StderrCancellation` 或 `StdinCancellation` 清理失败；对应
helper 可能一直存活到管道关闭。
成功取消后的交错核查及当前证据记录在 [RC-002 Windows 取消验证](rc-002-verification.md) 中。

输出截断与流完整性相互独立。完整流仍可能在内存中被截断，不完整流也可能只包含少量
字节。将保留字节当成完整 transcript 前，调用方必须同时检查这两类元数据。

## 错误优先级

主错误说明运行策略中第一个决定性失败。终止子进程或收尾 helper 时观察到的次级失败按
进程树、直接子进程、wait、时钟、stdout、stderr、stdin 的顺序放入 `cleanup_failures()`。
收尾采样时钟失败以 `Time` 保留；降级 helper 错误时同时移动可用输出。
`KillFailed` 和 `CancelFailed` 已移除，清理失败不再改变超时或取消的主错误类别。

tee 失败后，即使继续排空输出时发生取消或管道读取失败，已有 tee 错误也会保留。
同一流中先发生的 tee 错误优先于后续读取错误，两者降级到清理信息时仍完整保留。
捕获字节在 tee 写入之前保存；流完整性记录管道 EOF，因此 tee flush 失败不会将
已经排空的流标记为不完整。

准备、线程启动、timer 和进程控制错误可能不携带 `CommandOutput`。超时、取消、截断、
非预期退出、输出读取、tee 写入和最终 stdin 写入错误，在能够组装进程状态、耗时和流状态
时会携带输出。清理失败不会使部分输出变完整，也不表示所有后代都已停止。

监控仍存活的子进程时，检测到总预算超限会先于子进程状态、取消和超时检查；否则先检查子进程
状态，再检查取消与超时。子进程退出后的 I/O 收尾循环依次检查预算超限、取消、超时。reader
正常 join 后，其结果是该流的权威数据源；只有 helper 结果无法 join 时才使用预算快照，因此
既能保留部分输出，也不会丢弃已完成 reader 的结果。`total_output_limit_tests`、
`running_command_tests` 和 `output_collector_tests` 覆盖这些优先级及输出情况。

## 平台语义

Unix 使用 process group 和非阻塞管道轮询。文件型 stdin 与 tee 文件先使用非阻塞安全
标志打开，从活动句柄检查，再在交给子进程或 tee writer 前恢复阻塞模式；这样路径被替换
为 FIFO 时不会阻塞准备阶段。

Windows 在配置可用时使用 Job Object 管理进程树，管道句柄保持同步 I/O 语义，并通过
`CancelSynchronousIo` 作为正常 helper 中断机制。该调用失败会通过类型化 cancellation
cleanup failure 和上述共享的 100 ms 确认截止时间暴露出来；该时间只有请求失败时才会检查，且
不会为每个 helper 重置。它不限制后续 join 或整个调用，也不保证操作系统立即关闭管道。当前核查状态记录在 [RC-002
Windows 取消验证](rc-002-verification.md) 中。

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
