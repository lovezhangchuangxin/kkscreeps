# 调研底稿

本目录保存源码分析、社区项目研究和技术实验的历史证据。报告结论适用于记录的版本、环境和场景；它们是决策依据，不是当前实现或完整系统收益的证明。已采用的方案见[决策记录](../decisions/)，当前机制与目标设计见[架构文档](../architecture/)。

01–04 对应最初三轮调研（第一轮 = 01 与 02，第二轮 = 03，第三轮 = 04）；05 是报告编写时的 Mod 扩展能力专项复查；06 在用户明确 JS-only 后复审整体架构，并以新鲜构建和 JS/native 交互实验提出实施路线；07 调查注释的可读性与事实准确性，并提出规范和整改方案。建议按问题选择阅读；`notes/` 是报告的原始证据，报告中关键结论可回溯到出处。

| 报告 | 问题 | 报告结论与适用范围 |
|---|---|---|
| [01 可行性研究](01-screeps-rust-feasibility.md) | Rust 重构可行吗？收益多大？ | 支持 Rust 重写方向；序列化、存储和任务等待是候选优化点，整体收益预估仍需正式对照 |
| [02 沙箱选型](02-sandbox-evaluation.md) | 哪个 JS 引擎能承载玩家代码？ | 实验支持选择 deno_core（V8）；QuickJS 有条件可行，Boa 未满足选型要求，均不代表完整游戏 API 验收 |
| [03 架构深挖](03-architecture-optimizations.md) | 官方还有哪些可优化的地方？ | 提供序列化、查询等局部实验与架构建议；单项倍率不能代表完整系统收益 |
| [04 游戏功能完整性](04-game-api-feasibility.md) | deno_core 能实现游戏的全部功能吗？ | 语言与宿主交互实验支持实现方向；寻路回调实验可行，生产玩家接入和完整功能验收尚未完成 |
| [05 Mod 扩展能力](05-mod-extensibility-review.md) | 当前架构能达到官方与本地 xxscreeps 的 Mod 扩展能力吗？ | 当前尚未提供完整 Mod 通路；技术方向可保留，需要前移对象、意图、规则替换和事务等扩展契约 |
| [06 架构与 JS Mod 复审](06-architecture-and-js-mod-review.md) | 整体架构应如何改造，深度玩法能否全部用 JS Mod 实现？ | 保留 Rust/V8/房间模型，补全全域提交、并发派发、开放规则；JS/native 嵌套调用实测可行，完整实现仍待切片 |
| [07 注释质量调研](07-comment-quality-review.md) | 现有注释为什么难读，怎样避免误导维护者？ | 修复过期说明，分离历史与当前行为，形成[注释规范](../contributing/comment-style.md)和[术语词典](../glossary.md) |

## notes/ 笔记索引

**沙箱调研**（02 报告证据）：[sandbox-denocore.md](notes/sandbox-denocore.md)、[sandbox-quickjs.md](notes/sandbox-quickjs.md)、[sandbox-boa.md](notes/sandbox-boa.md)、[sandbox-alternatives.md](notes/sandbox-alternatives.md)

**沙箱实测**（02/04 报告证据，对应 `spikes/` 下可复跑的实验工程）：[spike-deno_core.md](notes/spike-deno_core.md)（中断/内存/性能基准）、[spike-rquickjs.md](notes/spike-rquickjs.md)、[spike-boa.md](notes/spike-boa.md)、[spike-deno-game-api.md](notes/spike-deno-game-api.md)（游戏 API 依赖的交互模式六组实验 + 指令回调实验 + 预算桶仿真）、[spike-deno-inspector.md](notes/spike-deno-inspector.md)（V8 inspector 断点调试：CDP 全链路 / 暂停×看门狗 / 全局面 diff / 快照 / 多 isolate 多 target）；另有 [rquickjs-run-output.txt](notes/rquickjs-run-output.txt)（rquickjs 安静负载轮的原始输出存档）

**架构深挖**（03 报告证据）：[arch-processor.md](notes/arch-processor.md)（引擎结算热路径）、[arch-runner.md](notes/arch-runner.md)（玩家运行时固定开销）、[arch-xxscreeps.md](notes/arch-xxscreeps.md)（社区重写项目的增量设计）、[arch-storage-backend.md](notes/arch-storage-backend.md)（存储与网络层，含官方闭源存储组件的解包实证）、[arch-community.md](notes/arch-community.md)（社区与官方一手情报）

**功能盘点**（04 报告证据）：[game-api-inventory.md](notes/game-api-inventory.md)（从中文文档站盘点的完整玩家功能面）、[deno-game-impl-mapping.md](notes/deno-game-impl-mapping.md)（功能组到沙箱能力的逐项映射）

**实现复查**（2026-09-28 优化轮证据）：[m0-m1-code-quality-audit.md](m0-m1-code-quality-audit.md)

**Mod 扩展实测**（05 报告证据，2026-10-01）：[复现脚本](notes/mod-extensibility-probe-2026-10-01.py)、[探针结果](notes/mod-extensibility-probe-2026-10-01.json)（临时内存世界的 Generic 管理查询、玩家对象字段与自定义 FIND 观察）

**整体架构与 JS Mod 新实验**（06 报告证据，2026-10-01）：[当前构建性能采集](notes/architecture-perf-probe-2026-10-01.py)、[12 次原始性能结果](notes/architecture-perf-probe-2026-10-01.json)；[JS 宿主采集](notes/js-mod-host-probe-2026-10-01.py)、[5 次 release + debug 结果](notes/js-mod-host-probe-2026-10-01.json)；[Rust 宿主原型](../../spikes/deno-spike/examples/js_mod_host.rs)、[JS 行为原型](../../spikes/deno-spike/examples/js_mod_host.js)。均为研究证据，非生产 Mod 实装或正式性能验收。

**架构再思考复核**（ADR-014 证据，2026-10-01）：[architecture-rethink-2026-10-01.md](notes/architecture-rethink-2026-10-01.md)（对 05/06 断言的双路独立核实、xxscreeps 机制取证要点、定型结论与数字勘误表）

**存储选型基准**（2026-09-29 M2-C3 骨架证据）：[storage-bench.md](notes/storage-bench.md)（redb vs libmdbx 微基准方法学、三档场景夹具、macOS arm64 数字（初轮 + 评审清偿后复跑）与选型判据草案——非正式选型，正式选型待 2A 后真实负载）

**文档历史**：[兼容性历史登记](notes/compatibility-register-history.md)、[路线图历史记录](notes/roadmap-history.md)保存正文整理前的完整记录，便于追溯旧结论；现行要求见[兼容性说明](../designs/compatibility.md)和[路线图](../roadmap.md)。

## 阅读注意

1. 报告中的结论按置信级别标注：**【实测】**（有可复现的实验或命令输出）、**【文档】**（有官方文档或源码出处）、**【推断】**（实测分量的线性外推或架构推理）——引用数字时请连同级别一起引用；
2. 02 报告的原始建议是"deno_core 主力 + QuickJS 备选档"，后来用户拍板放弃备选档，见 [ADR-002](../decisions/ADR-002-deno-core-sandbox.md)——报告保持历史原样，不做追改；
3. 笔记里的实验记录了精确的依赖版本、命令与原始输出，复跑方法见各 spike 工程目录。
