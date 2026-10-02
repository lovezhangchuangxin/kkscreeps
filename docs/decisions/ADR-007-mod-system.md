# ADR-007 用 JS 提供完整 Mod 扩展能力

- **日期**：2026-09-27
- **需求修订**：2026-10-01
- **状态**：已接受；架构与实施方向由 ADR-014 细化

## 背景

官方私服 Mod 通过 JS 模块、内部事件与 driver 接口扩展服务，常与 Node 进程及数据库内部结构耦合。逐接口兼容这些内部实现会限制本项目的数据、存储与调度重构。

## 决策

自建 Mod 接口，不要求官方 Mod 源码或内部 API 兼容。Mod 作者只编写 JS，不要求编写 Rust；扩展目标涵盖官方私服与调研版本 xxscreeps 的同类业务能力。

能力范围包括玩法规则、对象、动作、玩家 API、资源、地图、存储、网络、管理和客户端图形。不能以修改核心 Rust 源码代替安装式扩展，也不能把 JS 限制为统计和轻量管理。

默认 vanilla 配置保持 [ADR-003](ADR-003-player-compatibility.md) 的玩家契约。服主显式启用的玩法 Mod 可以改变规则，须发布能力与内容版本；Mod SDK 的兼容与客户端协议验收分别处理。

## 架构与实现边界

Rust 保留默认规则实现和宿主服务，向 JS 开放具名规则替换、事务状态、数据模型及外部服务入口。可信玩法 Mod 与不可信玩家实例分离；Mod 包也可贡献玩家 API 与客户端代码。

[ADR-014](ADR-014-contribution-plane.md)已确认贡献计划、默认玩法同契约、共享投影、全域候选、首版共同可信玩法 VM 和实施方向。本文不再把这些已确认方向写作待定；SDK 名称和具体接口仍按[Mod 设计](../designs/mod-system.md)与[实施方案](../plans/mod-plane-batches.md)逐项实现和验收。

当前尚未提供完整 Mod 加载与运行通路。“安装并重启生效”是目标使用方式，不能由已有 Generic 对象、事件观察或 JS/native 交互实验证明全部扩展能力成立。

## 修订关系

2026-09-27 曾提出 Rust 插件负责深度玩法、JS 负责管理与统计的双层方案。2026-10-01 明确 Mod 作者只写 JS 后，该语言分工与 JS 能力限制撤回；默认内核由 Rust 实现的选择保留。

## 理由与证据

统一贡献接口使默认行为与扩展行为共用规则入口，避免只能观察事件、无法替换玩法的局限。覆盖范围与真实改造成本仍须逐类验收。

- [可行性研究](../research/01-screeps-rust-feasibility.md)：官方 Mod 机制。
- [xxscreeps 调研](../research/notes/arch-xxscreeps.md)：贡献注册与加载机制。
- [扩展能力复查](../research/05-mod-extensibility-review.md)、[JS Mod 架构复审](../research/06-architecture-and-js-mod-review.md)：缺口与交互实验。
