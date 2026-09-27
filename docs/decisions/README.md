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
