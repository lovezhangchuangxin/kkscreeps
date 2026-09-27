# ADR-009 前端栈与整体 monorepo 形态

- **日期**：2026-09-27
- **状态**：已接受（用户指定技术栈并确认包划分方案）

## 背景

此前架构讨论覆盖了服务端（引擎、沙箱、存储），前端未定。用户明确要求：Vue 3 + Vite + TypeScript + pnpm；前端同样按 monorepo 组织；渲染层独立成包方便他人复用；**不复用官方渲染器**（其技术栈老旧——旧版 PixiJS + 自研表达式引擎），用最新 PixiJS 与更好的架构自建。

## 决策

1. **单 monorepo**：Rust workspace（crates/、compat/）与 pnpm workspace（apps/、packages/）共存于仓库根。不引入 turborepo/nx，先用根 package.json 脚本 + Vite。
2. **技术栈**：Vue 3 组合式 API + Vite + TypeScript + pnpm；渲染用 PixiJS 最新大版本（v8+，WebGPU/WebGL 双后端，实现期锁版本）；Vitest 单测，Playwright 按需。
3. **包划分**（详见 [frontend.md](../architecture/frontend.md)）：
   - `@kkscreeps/shared`：常量（从 Rust kk-core 生成，单一事实来源）+ 协议类型；
   - `@kkscreeps/renderer`：框架无关的渲染引擎（独立发布，React/Node 工具可复用）；
   - `@kkscreeps/api`：类型化网络客户端（浏览器/Node 通用）；
   - `@kkscreeps/editor`：代码编辑器 Vue 组件（内核已定 Monaco：智能提示对写 TS 的玩家价值大，包体问题用按需加载解决）；
   - `apps/game`：客户端应用，构建产物经 rust-embed 嵌入服务器二进制（单二进制自带 Web 客户端）。
4. **渲染器架构纪律**：框架无关、数据驱动（对象外观由元数据描述）；渲染循环归渲染器所有，Pixi 对象绝不进 Vue 响应式系统；输入接口（快照/增量/玩家绘制）与后端变更集同构，同一接口服务实时/回放/本地模拟三种数据源。
5. **资产复用**：官方开源美术（全套 SVG，宽松许可）继续复用；"数据驱动渲染元数据"的设计思想（含服务端注入扩展点）保留；官方渲染器的代码不移植。
6. **路线图影响**：M3 交付改为"自建客户端可玩 + 官方客户端可连接"；协议兼容关卡不变。

## 理由

- 渲染器独立成包 + 框架无关，是"方便其他人使用"的必要条件（社区可拿去做 bot 可视化，不必装 Vue）；
- 官方渲染器的旧栈（旧 PixiJS、自研表达式引擎）继续维护等于背上技术债，而其可取之处（数据驱动、SVG 资产、服务端元数据扩展点）都可以在新架构里保留；
- 常量与协议 DTO 都走"Rust 侧生成 + CI 新鲜度校验"的管线（DTO 生成管线已定采纳，契约测试作补充），消灭"同一概念两处定义"这个前后端协作的最大隐患；
- 客户端嵌入二进制保持"部署 = 一个文件"的项目卖点。

## 证据

- 官方渲染器技术栈与设计：`sources/screeps/renderer/`（PixiJS 版本与表达式引擎见 [01 报告](../research/01-screeps-rust-feasibility.md) §0 与 renderer 调研）
- 官方美术许可：`renderer/LICENSE.txt`（宽松许可，含全套 SVG，2026-09-27 已核）
- 详细设计：[frontend.md](../architecture/frontend.md)
