# kk-vm

玩家沙箱与可信玩法 VM 共用的执行机制，不包含游戏规则或脚本 API。

- `cpu_clock`：macOS/Linux 执行线程 CPU 时钟。
- `watchdog`：常驻墙钟看门狗，保留代次与解除武装后的中断投递纪律。
- `budget`：在 V8 安全点与调用返回时检查 CPU、墙钟和 V8 内存；故障保持毒化。

普通看门狗仍每代仅投递一次墙钟中断。预算监控模式到期后留存墙钟故障，并每 1 ms 持续检查/中断直到解除武装；deno_core 的异常转换会临时取消终止，错误格式化钩子仍可能执行 JS，这个窗口同样受限。投递与 disarm 共用代次锁，旧轮到期不能中断下一轮。

预算监控使用 V8 中断回调。构造 `VmBudget` 的宿主必须满足 unsafe 构造契约：看门狗在 isolate 前销毁，`keepalive` 的状态句柄保持到 isolate 销毁之后，调用与 isolate 同线程。`kk-mod-host::ModRealm` 用字段销毁顺序维持此纪律。

当前机制与资源边界见 [Mod 执行](../../docs/architecture/mod-execution.md#资源预算)。
