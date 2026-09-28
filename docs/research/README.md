# 调研底稿

本目录存放三轮调研的完整记录：对官方源码的剖析、对社区重写项目的研究、对各技术方案的实验验证。**这里的结论是决策的依据，不是决策本身**——定案见 [docs/decisions](../decisions/)，落地设计见 [docs/architecture](../architecture/)。

四份主报告对应三轮调研（第一轮 = 01 与 02，第二轮 = 03，第三轮 = 04），层层递进，建议按编号阅读；`notes/` 是报告的原始证据（调研笔记与实验记录），报告中所有关键结论都能回溯到笔记里的出处。

| 报告 | 问题 | 一句话结论 |
|---|---|---|
| [01 可行性研究](01-screeps-rust-feasibility.md) | Rust 重构可行吗？收益多大？ | 可行；收益大头来自架构（序列化/存储/进程间通信/串行等待），保守估计整体开销降一个数量级 |
| [02 沙箱选型](02-sandbox-evaluation.md) | 哪个 JS 引擎能承载玩家代码？ | deno_core（V8）十项硬要求基本全绿且全部实测；QuickJS 有条件可行；Boa 出局 |
| [03 架构深挖](03-architecture-optimizations.md) | 官方还有哪些可优化的地方？ | 把 01 的收益判断从推理升级为实测锚定（单项最高 80 倍），产出七项架构定型要求 |
| [04 游戏功能完整性](04-game-api-feasibility.md) | deno_core 能实现游戏的全部功能吗？ | 能；从官方中文文档盘点的全部功能面逐类验证，最后一个高风险缺口（寻路回调）实测关闭 |

## notes/ 笔记索引

**沙箱调研**（02 报告证据）：[sandbox-denocore.md](notes/sandbox-denocore.md)、[sandbox-quickjs.md](notes/sandbox-quickjs.md)、[sandbox-boa.md](notes/sandbox-boa.md)、[sandbox-alternatives.md](notes/sandbox-alternatives.md)

**沙箱实测**（02/04 报告证据，对应 `spikes/` 下可复跑的实验工程）：[spike-deno_core.md](notes/spike-deno_core.md)（中断/内存/性能基准）、[spike-rquickjs.md](notes/spike-rquickjs.md)、[spike-boa.md](notes/spike-boa.md)、[spike-deno-game-api.md](notes/spike-deno-game-api.md)（游戏 API 依赖的交互模式六组实验 + 指令回调实验 + 预算桶仿真）；另有 [rquickjs-run-output.txt](notes/rquickjs-run-output.txt)（rquickjs 安静负载轮的原始输出存档）

**架构深挖**（03 报告证据）：[arch-processor.md](notes/arch-processor.md)（引擎结算热路径）、[arch-runner.md](notes/arch-runner.md)（玩家运行时固定开销）、[arch-xxscreeps.md](notes/arch-xxscreeps.md)（社区重写项目的增量设计）、[arch-storage-backend.md](notes/arch-storage-backend.md)（存储与网络层，含官方闭源存储组件的解包实证）、[arch-community.md](notes/arch-community.md)（社区与官方一手情报）

**功能盘点**（04 报告证据）：[game-api-inventory.md](notes/game-api-inventory.md)（从中文文档站盘点的完整玩家功能面）、[deno-game-impl-mapping.md](notes/deno-game-impl-mapping.md)（功能组到沙箱能力的逐项映射）

**实现复查**（2026-09-28 优化轮证据）：[m0-m1-code-quality-audit.md](m0-m1-code-quality-audit.md)

## 阅读注意

1. 报告中的结论按置信级别标注：**【实测】**（有可复现的实验或命令输出）、**【文档】**（有官方文档或源码出处）、**【推断】**（实测分量的线性外推或架构推理）——引用数字时请连同级别一起引用；
2. 02 报告的原始建议是"deno_core 主力 + QuickJS 备选档"，后来用户拍板放弃备选档，见 [ADR-002](../decisions/ADR-002-deno-core-sandbox.md)——报告保持历史原样，不做追改；
3. 笔记里的实验记录了精确的依赖版本、命令与原始输出，复跑方法见各 spike 工程目录。
