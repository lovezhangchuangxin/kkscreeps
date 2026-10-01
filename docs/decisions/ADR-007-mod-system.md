# ADR-007 JS mod 系统自建，追求功能扩展能力

- **日期**：2026-09-27
- **需求修订**：2026-10-01（用户再次确认）
- **状态**：已接受（用户拍板）

## 背景

官方私服有一套 mod 机制：JS 模块被 `require` 进各个 Node.js 进程，通过 `config.engine` 等内部事件钩子和 driver 方法扩展服务器，社区有 `screepsmod-*` 系列 мод。早期讨论中曾考虑"事件命名尽量对齐官方以降低 mod 移植成本"的折中。

这些 mod 与官方实现深度耦合：直接操作官方进程内部结构和 MongoDB/lokijs 数据库集合——而这两样正是本项目重构掉的东西。

## 决策

**不要求官方 mod 代码/API 兼容，自建 mod 机制；Mod 作者编写 JS，不编写 Rust。** 扩展能力目标是能够实现官方私服与本地 xxscreeps 的同类 Mod 功能，包括深度玩法、对象、动作、玩家 API、资源、地图、存储、网络、管理与客户端图形。不能以“修改核心 Rust 源码”代替 Mod 能力，也不能把 JS 层限制为统计和轻量管理。

兼容红线覆盖默认 vanilla 玩家侧（见 [ADR-003](ADR-003-player-compatibility.md)）；服主明确启用的玩法 Mod 可以改变行为，应发布相应能力与版本。Mod API 自建与必要的客户端协议适配是独立问题。

## 实现方向（具体接口仍为提案）

1. **Rust 内核与宿主服务**：保留高效默认实现，向 JS 提供具名规则替换、事务状态、数据模型和外部服务入口。这是引擎实现，不是 Mod 作者的交付语言。
2. **可信 JS Mod**：与玩家 V8 实例分离，通过统一贡献定义新增、包裹、取消或替换玩法及服务；包的玩家 API 与客户端部分也是 JS，安装并重启可生效。

设计草案见 [mod-system.md](../designs/mod-system.md)，整体改造建议与实验见 [06 架构复审](../research/06-architecture-and-js-mod-review.md)。**本 ADR 确认语言和能力需求，不把草案的 SDK 名称、crate 拆分、线程模型或批次视为已实现/已接受。** 核心扩展边界前移的建议由该调研提出，完整排期仍需按具体实施计划调整。

**后续（同日）**：架构骨架已由 [ADR-014](ADR-014-contribution-plane.md) 定型（贡献平面六原则、四项实施拍板、封闭分派评审门禁；批次方向 A–G 及四处调整亦经其决策 4 接受，切片见 [mod-plane-batches](../plans/mod-plane-batches.md)）；本文「实现方向」升格为定型架构的具体设计输入，上文「不把批次视为已接受」就批次方向而言已被 ADR-014 取代。

## 历史方向

2026-09-27 曾提出“Rust 编译期插件负责深度玩法 + JS 管理/统计”的双层方向。2026-10-01 用户明确 Mod 必须是 JS，该方向中对 Mod 作者使用 Rust 及 JS 能力限制的部分已撤回。默认玩法由 Rust 实现仍成立。

## 理由

官方 mod 与 Node/数据库内部实现耦合，逐接口兼容会限制重构。采用贡献点模式和 JS 宿主可以提供安装式扩展，并保留 Rust 默认内核；需要开放完整业务领域与默认行为替换，单纯事件观察不足以满足用户需求。

## 证据

- 官方 mod 机制：`common/lib/config-manager.js`、`launcher/init_dist/example-mods/`（调研见 [01 报告](../research/01-screeps-rust-feasibility.md)）
- xxscreeps 贡献点模式：[arch-xxscreeps.md](../research/notes/arch-xxscreeps.md)
