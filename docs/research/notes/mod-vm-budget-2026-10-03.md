# Mod VM 资源预算（2026-10-03）

基线 `471ac1d` 已有固定快照恢复，但纯 JS 无限循环与过量 V8 分配仍能阻止正常错误返回。本片为注册和结算增加 VM 总预算，限额故障触发既有候选失败与销毁/恢复路径。

## 实现边界

- 新增 `kk-vm`：从 runner 提取已存在的 CPU 时钟与常驻墙钟看门狗，runner 保留原导入路径；host 与 runner 共用通用 V8 执行机制，无需相互依赖。core 与 engine 不依赖 V8。
- CPU 使用执行线程累计时间。安全点每 1 ms 检查 CPU 与 V8 已用堆加堆外计数；同步返回再检查。墙钟看门狗独立 terminate。
- 注册整体限额与整轮 Mod 调用累计限额分开。每个槽、目标和房间只用剩余额度，Rust/native op 的调用内成本计入；纯 Rust 规则在两次 Mod 调用之间的执行不计入。
- 近堆回调请求终止并提供有限传播余量。预算故障留存至 VM 销毁，不把中断后的 VM 清理标志再复用。
- runtime 原包、配置和预算一起保存在启动快照，重建使用同一配置并核验原规则计划。预算配置独立于规则 PlanHash，注册选项与构造配置必须一致。
- 监控回调原始地址由 Arc 固定，realm 的字段销毁顺序为看门狗 → JsRuntime → 最后一个状态 keepalive；不引入 Working 指针或跨调用候选借用。

默认 CPU/墙钟限额：注册 500 ms / 2 s，结算累计 100 ms / 1 s，V8 内存 128 MiB。它们是可调的初始保护配置，不是密集 Mod 性能验收结论。完整语义见 [当前机制](../../architecture/mod-execution.md#资源预算)。

## 验收场景

- 子进程故障矩阵：模块顶层与注册回调 CPU 死循环、墙钟死循环、连续 Promise 微任务、before/wrap/center/after CPU 死循环、数组堆分配、单次 ArrayBuffer 堆外分配与多次短调用累计超限。父测试设 10 秒独立期限，失败实现不会无限挂住测试。
- 等待原生 op 不消耗等量 CPU；返回时排队的安全点检查和下一次调用按当前预算判断。
- 配置非法在 VM 创建前拒绝；配置不匹配的注册失败仍密封 realm。
- JSON/KKPV runtime：native 和 namespace 已写候选后 CPU 超时，世界、玩家 Memory/CPU 与代际保持旧值；重试忽略新代码，使用原预算与缓存结果；显式丢弃后新指令可提交，玩家全局计数证明旧失败重试未重跑。
- JSON/KKPV runtime：ArrayBuffer 超限使 native 与 Mod 写入全部失败，连续重建不残留超量全局。
- 400 个对象的短回调共享整轮额度；单个回调可完成，累计耗尽后所有对象与 namespace 写入均不提交。

## 限制

此机制有采样与线程调度延迟，V8 中断不能强制取消 Rust/native 阻塞；单次巨大分配可能先于检查失败，进程级 OOM 仍可能中止。Rust 内存、未被 V8 统计的外部内存、进程 RSS 与逐 Mod 归因不在当前计数内。预算不是完整 OS 资源隔离；进程级约束、故障停用与持久化仍待后续。

## 最终验证

- `pnpm run check`：格式、workspace/all-targets Clippy、554 个 Rust 测试与 19 个 JavaScript 测试通过。8 个 Rust 测试标为 ignored；其中资源矩阵助手由父测试在子进程中显式运行，并非漏验故障场景。
- 重建 release CLI 与适配器后运行固定版本 screeps-ok 棘轮：1854 passed / 153 failed / 907 skipped，满足 1854 下限；相对验收时 `HEAD~1`（`f1db17c`）转红 0、转绿 0。
- tooangel / overmind / wasm 分别完成 2000 / 2000 / 10 tick，均无终止、惩罚跳过或 missing API。overmind 仍有 1 个 console-error tick；tooangel 仍记录已有的 `offsetPosition.lookFor` 错误。此处仅验证玩家 VM 回归，Mod 预算由 host 故障矩阵与 runtime 测试覆盖。
- 构建产物摘要、具体矩阵与 bot 原始报告见[机器验收记录](mod-vm-budget-validation-2026-10-03.json)。本片未做密集 Mod 性能基准，不据此调整默认限额。
