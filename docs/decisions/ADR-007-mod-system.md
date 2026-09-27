# ADR-007 mod 系统自建，不兼容官方 mod 生态

- **日期**：2026-09-27
- **状态**：已接受（用户拍板）

## 背景

官方私服有一套 mod 机制：JS 模块被 `require` 进各个 Node.js 进程，通过 `config.engine` 等内部事件钩子和 driver 方法扩展服务器，社区有 `screepsmod-*` 系列 мод。早期讨论中曾考虑"事件命名尽量对齐官方以降低 mod 移植成本"的折中。

这些 mod 与官方实现深度耦合：直接操作官方进程内部结构和 MongoDB/lokijs 数据库集合——而这两样正是本项目重构掉的东西。

## 决策

**不做官方 mod 兼容**（连接口对齐也不做），自建 mod 机制，保证将来能基于它开发 mod 即可。兼容红线只覆盖玩家侧（见 [ADR-003](ADR-003-player-compatibility.md)），mod 属于服务器管理侧，不受红线约束。

## 自建机制的两层形态（方向）

1. **Rust 编译期插件**：参考 xxscreeps 的"贡献点合并"模式（玩法本身拆成可插拔模块，通过声明式注册合并进引擎）——适合深度扩展玩法、改结算逻辑；
2. **运行时 JS mod**：跑在高权限的独立 V8 实例（比玩家沙箱权限高、仍与引擎核心隔离），通过稳定的引擎事件与扩展点 API 交互——适合管理工具、统计、轻量玩法调整，不需要重新编译服务器。

API 设计不受官方 mod 接口的历史约束，按本项目架构自由设计；设计文档待补（见 designs 目录规划清单，排 M4）。

## 理由

官方 mod 生态绑死 Node 进程内部，逐接口兼容等于把要重构的东西照搬回来；且官方 mod 本就随版本内部变动频繁失效，生态迁移价值有限。自建机制可以用贡献点模式获得更干净的玩法可插拔性（xxscreeps 已验证该路线），同时保留运行时 JS mod 的便利性。

## 证据

- 官方 mod 机制：`common/lib/config-manager.js`、`launcher/init_dist/example-mods/`（调研见 [01 报告](../research/01-screeps-rust-feasibility.md)）
- xxscreeps 贡献点模式：[arch-xxscreeps.md](../research/notes/arch-xxscreeps.md)
