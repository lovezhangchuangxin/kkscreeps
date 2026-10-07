# ADR-002 玩家代码沙箱采用 deno_core

- **日期**：2026-09-27
- **状态**：已接受；取代选型报告中保留 QuickJS 备选后端的建议

## 背景

玩家代码需要独立实例、跨 tick 状态、多模块加载、CPU 预算、中断和内存控制。选型实验比较了 deno_core、QuickJS 与 Boa，并检查游戏 API 所需的宿主调用与回调方式。

## 决策

只采用 deno_core 作为玩家 JavaScript 沙箱后端，不实现 QuickJS 或 Boa 后端。一个玩家实例绑定一个线程；引擎通过执行接口使用沙箱，不直接依赖 V8。

## 理由与证据边界

- deno_core 使用 V8，具备中断、启动快照、宿主函数和嵌套回调等所需机制。选型实验验证了这些具体路径，完整玩家 API 和生产隔离仍须另外验收。
- QuickJS 在记录的 fib、对象分配和 JSON 负载中较慢，倍率随场景变化。CPU 预算会使执行成本影响玩家工作量；不维护第二后端可减少行为与计费差异。
- 选型时测试的 Boa 版本缺少满足要求的实例内存限额机制。该结论适用于报告版本，不代表未来版本的能力。

同用 V8 有助于语言兼容，但不保证版本、启动选项、内建全局和执行性能完全相同。快照创建、简单宿主调用和回调的实验数字也不能外推为生产 API 成本。

## 使用约束

同一线程内的 V8 Enter/Exit 栈须按正确顺序管理；不存在所有玩家线程之间的全局逆序销毁要求，见 [ADR-013](ADR-013-architecture-review-coordinator.md)。当前实例管理与回收接口见[沙箱说明](../architecture/sandbox.md)。

堆上限须结合 near-heap 回调处理中断和临时抬限，避免直接进入进程级 FatalOOM。该机制不能覆盖全部堆外分配或原生调用；ArrayBuffer、WebAssembly、宿主分配与进程资源仍须分别控制。

启动时应移除不需要的宿主入口，控制句柄留在 Rust 私有状态中。V8 实例隔离不能代替公网部署所需的进程隔离与资源限制。依赖版本应锁定，并按 deno_core 与 V8 的实际集成关系验证安全更新。

## 证据

- [沙箱选型报告](../research/02-sandbox-evaluation.md)：采用版本、实验负载、硬要求与未覆盖项。
- [游戏功能实验](../research/04-game-api-feasibility.md)：游戏 API 依赖的交互方式。
- [deno_core 实验记录](../research/notes/spike-deno_core.md)及[实验工程](../../spikes/deno-spike)：中断、内存、快照和调用测试。
