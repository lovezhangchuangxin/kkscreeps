# 技术决策记录

技术决策记录（ADR，Architecture Decision Record）说明问题、采用的方案、取舍理由和证据。已接受的决策约束后续设计；改变决策时记录替代关系，保留原来的背景与依据。

“已接受”表示方案已确认，不表示功能已实现。当前机制见[架构文档](../architecture/README.md)，能力与进展见[项目说明](../../README.md)和[路线图](../roadmap.md)。后续决策对早期方案的调整按下面的关系阅读。

## 决策索引

| 编号 | 决定的事项 | 后续关系与阅读提示 |
|---|---|---|
| [ADR-001](ADR-001-rust-monorepo.md) | Rust 服务端与 Cargo workspace | 模块现状见总体架构；性能收益由端到端基准验证 |
| [ADR-002](ADR-002-deno-core-sandbox.md) | 只采用 deno_core 玩家沙箱 | ADR-013 澄清线程局部销毁约束；安全与计费边界见沙箱文档 |
| [ADR-003](ADR-003-player-compatibility.md) | 默认玩家行为兼容优先于性能优化 | ADR-007/014 明确默认玩法与启用 Mod 的验收边界 |
| [ADR-004](ADR-004-core-architecture.md) | 快照、索引、流水线与分层存储 | ADR-013/014 调整投影协议、实施顺序和提交边界 |
| [ADR-005](ADR-005-dev-kit.md) | 开发套件共用服务器引擎二进制 | 完整套件仍为目标，具体使用流程见功能设计 |
| [ADR-006](ADR-006-screeps-ok-ci.md) | screeps-ok 进入 CI 验收 | 当前报告、预期差异与回归豁免规则见[验收工具](../../compat/README.md) |
| [ADR-007](ADR-007-mod-system.md) | Mod 作者使用 JS，自建扩展接口 | 已撤回 Rust 作者插件方向；架构由 ADR-014 定型 |
| [ADR-008](ADR-008-runtime-model.md) | 执行、线程、失败处理与资源管理 | ADR-013/014 补充协调与全域提交；原文的调度和回收策略不全是现状 |
| [ADR-009](ADR-009-frontend-stack.md) | Vue、Vite、TypeScript、pnpm 与前端包边界 | 当前包为占位，生成、构建与协议仍须实现 |
| [ADR-010](ADR-010-screeps-ok-adapter-bridge.md) | 适配器、嵌入接口和 M0 验收范围 | 早期哑世界已经由真实结算替代，保留当时的接口选择依据 |
| [ADR-011](ADR-011-engine-skeleton.md) | 类型化世界、结算骨架与回归检查 | M1 定型记录；投影目标随后由 ADR-014 演进 |
| [ADR-012](ADR-012-m2a-sandbox-runtime.md) | 沙箱接入、玩家 API 和指令通路 | M2A 定型记录；运行协调见 ADR-013，投影目标见 ADR-014 |
| [ADR-013](ADR-013-architecture-review-coordinator.md) | 运行协调、提交边界与时间输入 | 实装范围见 tick 契约；全域事务与并发仍有后续目标 |
| [ADR-014](ADR-014-contribution-plane.md) | 贡献定义、规则替换、共享投影与全域事务 | 冻结计划、默认玩法同契约及扩展入口要求；切片见贡献平面方案 |
| [ADR-015](ADR-015-mod-chain-execution.md) | JS 组合处理链与 Rust 候选访问 | 整槽进入 JS、native op 访问候选；当前机制见 Mod 执行文档 |
| [ADR-016](ADR-016-official-content-packs.md) | 官方玩法包、内容选择与公共 JS SDK 验收 | 细化 ADR-007/014/015：部分官方内容由 JS 实现，预设装配与存量迁移尚未实现 |

| [ADR-017](ADR-017-background-services.md) | 独立线程的异步后台服务域 | G27.a 首片已交付；耐久任务与 outbox 另有提交边界 |
| [ADR-018](ADR-018-production-persistence-posture.md) | 生产持久化=周期检查点权威，输入日志为显式审计件 | 技术基座随输入日志 v2 交付；服务侧接线在 G27.b/G28 线 |
| [ADR-019](ADR-019-generic-payload-metadata.md) | Generic 不可变载荷与紧凑读取元数据 | 减少跨消费者 JSON 分类解析；保留动态载荷与原二进制布局，候选与调度后续独立评估 |
| [ADR-020](ADR-020-incremental-world-runtime.md) | 拟议：统一变化集、紧凑房间存储与依赖调度 | 2026-10-11 全面重构推荐方案；实施和收益均待验证，保留完整 Mod 与原子提交 |
| [ADR-021](ADR-021-service-command-durability.md) | 拟议：周期检查点上的命令确认屏障与恢复下限 | 服务 S2 的耐久接收、最终确认、outbox 与安全回滚边界；尚未实现 |

ADR-001–019 已接受；ADR-020 为拟议重构方向，ADR-021 为拟议服务耐久方案。具体修订、实施状态与范围以各记录正文为准。

## 评估后未采用的方案

本表概括当时的取舍，不把旧版本实验结果视为永久能力判断。性能数字和机制限制应连同报告的版本、环境与负载阅读。

| 方案 | 当时未采用的原因 | 依据 |
|---|---|---|
| QuickJS 作为玩家沙箱备选 | 记录的负载较慢，预算制下影响玩家工作量；同时维护两套后端会增加行为差异 | [沙箱选型](../research/02-sandbox-evaluation.md)、ADR-002 |
| Boa 作为玩家沙箱 | 选型版本的实例内存与中断机制未满足要求 | [沙箱选型](../research/02-sandbox-evaluation.md) |
| wasmtime + Javy 编译玩家 JS | 评估方案无法保持 eval、new Function 等动态能力 | [备选方案笔记](../research/notes/sandbox-alternatives.md) |
| 直接使用 rusty_v8 | 需要自行补齐 deno_core 已提供的嵌入设施，评估时迁移成本较高 | [备选方案笔记](../research/notes/sandbox-alternatives.md) |
| isolated-vm 与 Node 伴生进程 | 选型实验支持 deno_core 方向，未采用过渡后端；不表示所有生产风险已关闭 | [游戏功能实验](../research/04-game-api-feasibility.md) |
| 重新实现寻路算法 | 同成本路径的选择仍可被玩家观察，优先复用并验证官方实现 | [兼容性要求](../designs/compatibility.md) |
| 移植官方渲染器代码 | 采用自建架构和独立包；资产复用另核对许可 | [ADR-009](ADR-009-frontend-stack.md) |
| 世界模型整体改为 ECS | 尚无足够收益证据，类型化对象与确定性迭代更符合当前需求 | [架构复审笔记](../research/notes/architecture-review-2026-09.md) |
| 微服务、每房常驻 Actor 或完整事件溯源 | 当前阶段成本较高，仍不能绕过跨房协调与统一提交要求 | [架构复审笔记](../research/notes/architecture-review-2026-09.md) |
| 在完整链路验证前冻结零拷贝布局 | 须先验证生命周期、权限与端到端收益；ADR-014 要求整体设计协议，具体实现仍分步验收 | [ADR-013](ADR-013-architecture-review-coordinator.md)、[ADR-014](ADR-014-contribution-plane.md) |
