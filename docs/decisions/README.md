# 决策记录（ADR）

这里存放本项目的重要技术决策记录（ADR，Architecture Decision Record）。每份记录回答四个问题：当时面对什么问题、决定了什么、为什么这么决定、有什么证据。这些决策一经确认就约束后续开发；要推翻某条决策，新写一份 ADR 并标注取代关系。

## 决策索引

| 编号 | 决策 | 状态 |
|---|---|---|
| [ADR-001](ADR-001-rust-monorepo.md) | 用 Rust + cargo monorepo 从零重构服务端 | 已接受 |
| [ADR-002](ADR-002-deno-core-sandbox.md) | 玩家代码沙箱只采用 deno_core，不做备选后端 | 已接受 |
| [ADR-003](ADR-003-player-compatibility.md) | 玩家侧体验零差异是最高优先级约束 | 已接受 |
| [ADR-004](ADR-004-core-architecture.md) | 核心架构定型：房间快照 + 流水线 + 索引 + 分层存储 | 已接受 |
| [ADR-005](ADR-005-dev-kit.md) | 开发套件与服务器共用同一个引擎二进制 | 已接受 |
| [ADR-006](ADR-006-screeps-ok-ci.md) | screeps-ok 行为测试套件进 CI 作为验收关卡 | 已接受 |
| [ADR-007](ADR-007-mod-system.md) | mod 系统自建，不兼容官方 mod 生态 | 已接受 |
| [ADR-008](ADR-008-runtime-model.md) | 运行时执行模型与资源管理（线程三层次/同步+并行/事务性结算/速率保护等八项） | 已接受 |
| [ADR-009](ADR-009-frontend-stack.md) | 前端栈（Vue3+Vite+TS+pnpm）与单 monorepo、自建渲染器 | 已接受 |
| [ADR-010](ADR-010-screeps-ok-adapter-bridge.md) | screeps-ok 适配器桥接（sidecar 子进程）与嵌入式 API、M0 哑世界、bot 验收与上游 pin 策略 | 已接受 |
| [ADR-011](ADR-011-engine-skeleton.md) | 引擎骨架定型：typed world 唯一后端、官方结算序子集、intent 通道 trait、声明宏快照格式、常量生成、通过率棘轮 | 已接受 |
| [ADR-012](ADR-012-m2a-sandbox-runtime.md) | M2A 架构定型：两条硬约束裁决器、玩家 API 层 prelude 对照移植、intent 链路四段、数据面分期 seam 先行、查询面 Rust 单源、验收口径、性能观测点（A6 收口升格） | 已接受 |
| [ADR-013](ADR-013-architecture-review-coordinator.md) | 2026-09 架构复审：运行协调层（RunResult 统一提交）、直读 seam 先行节奏、LIFO 每线程纠偏、宿主控制面收口、TickInput 非确定性显式化（ADR-012 由 A6 预留） | 已接受 |

## 评估过但没有采纳的方案

| 提案 | 否决原因 | 证据 |
|---|---|---|
| QuickJS（rquickjs 绑定）作为低内存备选档 | 单实例内存只要约 0.4MB 很诱人，但执行速度比 V8 慢约 12 倍——CPU 是预算制，这会改变玩家能完成的工作量，破坏公平性；用户拍板不做备选档 | [沙箱选型报告](../research/02-sandbox-evaluation.md) |
| Boa（纯 Rust 的 JS 引擎） | 没有"内存上限"接口：实验中一段扩张内存的代码让它多吃 216MB 无任何拦截 | 同上 |
| wasmtime + Javy（玩家代码编译成 WebAssembly） | 不支持 `eval`、`new Function` 等动态能力，玩家代码形态不兼容 | [备选方案笔记](../research/notes/sandbox-alternatives.md) |
| 直接用 rusty_v8 裸绑（不经 deno_core） | 底层引擎相同、性能相同，但要自己造启动快照、函数注入、模块加载全套设施，且 rusty_v8 的 API 还在不稳定期（调研中迁移实验撞了 19 个编译错误） | 同上 |
| isolated-vm + Node 独立伴生进程（过渡方案） | deno_core 的全部风险点已实测关闭，不需要过渡形态 | [游戏功能验证报告](../research/04-game-api-feasibility.md) |
| 自己实现寻路算法 | 即使结果"同样最优"，路径的细节选择（先走哪条同长度的路）可能和官方不同，玩家会观察到差异 | [兼容性红线](../designs/compatibility.md) |
| 复用官方渲染器代码（01 报告原建议） | 技术栈老旧（旧版 PixiJS + 自研表达式引擎），自建渲染器但复用其美术 SVG 资产与数据驱动设计思想 | [ADR-009](ADR-009-frontend-stack.md) |
| 世界模型整体改 ECS（2026-09 复审再裁决） | Screeps 对象异构、强类型、每房几百个、迭代序必须确定——`BTreeMap<ObjId, RoomObject>` + 格子索引够用；ECS 的 archetype 收益未证明且破坏"typed 即 schema 类型"单真源 | [架构复审笔记](../research/notes/architecture-review-2026-09.md) |
| 微服务化 / 每房常驻 Actor / 完整事件溯源（同上） | 当前阶段成本高；统一提交与跨房语义仍需协调，换存储接口不解决分布式执行语义；重放需求用 tick 输入日志 + 检查点即可覆盖 | 同上 |
| 立即冻结零拷贝列式快照布局（同上） | 偏早：应先验证完整读写链路、生命周期与端到端收益，再按 snapshot 契约的冻结前验收条款冻结；但 seam（世界来源接口化/地形缓存/不可变房间版本句柄）须先行，避免 M2B API 扩面按旧形态翻倍 | [ADR-013](ADR-013-architecture-review-coordinator.md) |
